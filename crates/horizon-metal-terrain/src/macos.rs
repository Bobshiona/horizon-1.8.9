//! Metal-only interop. Raw handles never escape this module. Every external
//! resource is retained until its unretained command buffer has completed.
use super::{Encoded, Frame, TerrainEncoder};
use objc2::{Message, rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSRange, NSString};
use objc2_metal::*;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wgpu::hal::api::Metal;
use wgpu::util::DeviceExt as _;

type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;
type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type Pipeline = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;
type Depth = Retained<ProtocolObject<dyn MTLDepthStencilState>>;
const MAX_PAGES: usize = 1024;
type Icb = Retained<ProtocolObject<dyn MTLIndirectCommandBuffer>>;

// SAFETY: all callers hold a live borrowed Objective-C object while retaining it.
unsafe fn retained<T: Message>(value: &T) -> Retained<T> {
    unsafe { Retained::retain(std::ptr::from_ref(value).cast_mut()).expect("live native object") }
}
fn raw_buffer(value: &wgpu::Buffer) -> Result<Buffer, String> {
    // SAFETY: inspect only this live Metal allocation, immediately retain it,
    // then release wgpu's guard before any further encoder/device access.
    unsafe {
        let hal = value
            .as_hal::<Metal>()
            .ok_or("Buffer is not a live Metal allocation")?;
        Ok(retained(hal.raw_handle()))
    }
}
fn raw_view(value: &wgpu::TextureView) -> Result<Texture, String> {
    // SAFETY: same lifetime/guard rules as raw_buffer, preserving the view format.
    unsafe {
        let hal = value
            .as_hal::<Metal>()
            .ok_or("View is not a live Metal allocation")?;
        Ok(retained(hal.raw_handle()))
    }
}
fn gpu_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawData {
    low: [f32; 4],
    high: [f32; 4],
    args: [u32; 4],
    page: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CullData {
    planes: [[f32; 4]; 5],
    count: [u32; 4],
}

struct Slot {
    culled: Buffer,
    commands: Icb,
    container: Buffer,
}
struct FrameSlot {
    inputs: wgpu::Buffer,
    raw: Arc<Slot>,
    capacity: usize,
}
// SAFETY: slots are immutable while referenced by deferred commands. Their
// native buffers are Metal thread-safe resource objects, not mutable encoders.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}
struct Snapshot {
    inputs: Buffer,
    slot: Arc<Slot>,
    buffers: Vec<Buffer>,
    texture: Texture,
    pipelines: [Pipeline; 3],
    depth: [Depth; 2],
    compute: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    _sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
    pages: Vec<Buffer>,
    offsets: [usize; 2],
    count: usize,
    passes: [std::ops::Range<usize>; 3],
    optimize: bool,
    occlusion: Option<OcclusionSnapshot>,
}
/// The depth pyramid for one viewport size: a mipmapped R32Float texture whose
/// level 0 covers the viewport rounded up to powers of two, and a view per level.
struct Pyramid {
    size: [u32; 2],
    texture: Texture,
    levels: Vec<Texture>,
}
// SAFETY: Metal textures are thread-safe objects; the pyramid's handles are never
// mutated on the CPU after creation, only written by GPU commands in order.
unsafe impl Send for Pyramid {}
unsafe impl Sync for Pyramid {}
struct OcclusionSnapshot {
    /// First command tested (the first solid command after the occluders).
    from: usize,
    occluders: std::ops::Range<usize>,
    depth: Texture,
    pyramid: Arc<Pyramid>,
    viewport: [u32; 2],
    pipelines: [Retained<ProtocolObject<dyn MTLComputePipelineState>>; 3],
    culled: Buffer,
}
// SAFETY: Snapshot owns immutable thread-safe Metal resources. It excludes CPU
// staging and argument encoders. encode borrows only the exclusively supplied HAL
// encoder and never mutates the resource contents on the CPU or re-enters wgpu.
unsafe impl Send for Snapshot {}
unsafe impl Sync for Snapshot {}
struct Hook {
    frame: Arc<Snapshot>,
    stage: u8,
}
impl std::fmt::Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainHook")
            .field("stage", &self.stage)
            .finish()
    }
}
impl wgpu::wgc::command::NativeCommand for Hook {
    unsafe fn encode(
        &self,
        encoder: &mut dyn wgpu::hal::DynCommandEncoder,
    ) -> Result<(), wgpu::hal::DeviceError> {
        let encoder = encoder
            .as_any_mut()
            .downcast_mut::<wgpu::hal::metal::CommandEncoder>()
            .expect("Metal terrain hook backend");
        let f = &self.frame;
        // SAFETY: callbacks were recorded in standalone / render contexts by the
        // owned Encoded wrapper; all pointed-to allocations live through the core
        // submission fence. Slots are never reused while snapshots retain them.
        unsafe {
            if self.stage == 0 {
                encoder.prepare_native_commands();
                let command = encoder
                    .raw_command_buffer()
                    .expect("open deferred Metal encoder");
                if f.count == 0 {
                    return Ok(());
                }
                // The kernel writes or resets every command in 0..count itself:
                // one compute encoder instead of a reset blit before it.
                let compute = command
                    .computeCommandEncoder()
                    .ok_or(wgpu::hal::DeviceError::OutOfMemory)?;
                compute.setComputePipelineState(&f.compute);
                let inputs = &f.inputs;
                for (slot, (buffer, offset)) in [
                    (0, (inputs, 0)),
                    (1, (inputs, f.offsets[0])),
                    (2, (&f.buffers[0], 0)),
                    (3, (&f.slot.container, 0)),
                    (4, (inputs, f.offsets[1])),
                    (5, (&f.buffers[1], 0)),
                    (6, (&f.buffers[2], 0)),
                    (8, (&f.buffers[3], 0)),
                ] {
                    compute.setBuffer_offset_atIndex(Some(buffer), offset, slot);
                }
                for page in &f.pages {
                    compute.useResource_usage(
                        ProtocolObject::from_ref(&**page),
                        MTLResourceUsage::Read,
                    );
                }
                compute.useResource_usage(
                    ProtocolObject::from_ref(&*f.slot.commands),
                    MTLResourceUsage::Write,
                );
                compute.dispatchThreads_threadsPerThreadgroup(
                    MTLSize {
                        width: f.count,
                        height: 1,
                        depth: 1,
                    },
                    MTLSize {
                        width: f.compute.threadExecutionWidth().min(f.count),
                        height: 1,
                        depth: 1,
                    },
                );
                compute.endEncoding();
                // Keep optimization regions separate: executeCommands may only
                // start at an optimized region's beginning. Combining passes
                // would cross pipeline / translucent ordering boundaries.
                // With occlusion only the occluders are final here; the rest are
                // optimized after the occlusion test rewrites them (stage 4).
                let ranges: &[std::ops::Range<usize>] = match &f.occlusion {
                    Some(occlusion) => std::slice::from_ref(&occlusion.occluders),
                    None => &f.passes,
                };
                optimize(command, f, ranges)?;
            } else if self.stage == 4 {
                encoder.prepare_native_commands();
                let command = encoder
                    .raw_command_buffer()
                    .expect("open deferred Metal encoder");
                let Some(occlusion) = &f.occlusion else {
                    return Ok(());
                };
                occlude(command, f, occlusion)?;
                let ranges = [
                    occlusion.from..f.passes[0].end,
                    f.passes[1].clone(),
                    f.passes[2].clone(),
                ];
                optimize(command, f, &ranges)?;
            } else {
                let render = encoder
                    .raw_render_encoder()
                    .expect("active deferred Metal render pass");
                render.setFrontFacingWinding(MTLWinding::CounterClockwise);
                render.setTriangleFillMode(MTLTriangleFillMode::Fill);
                render.setDepthClipMode(MTLDepthClipMode::Clip);
                render.setDepthBias_slopeScale_clamp(0.0, 0.0, 0.0);
                render.useResource_usage_stages(
                    ProtocolObject::from_ref(&*f.texture),
                    MTLResourceUsage::Read,
                    MTLRenderStages::Fragment,
                );
                for buffer in &f.buffers {
                    render.useResource_usage_stages(
                        ProtocolObject::from_ref(&**buffer),
                        MTLResourceUsage::Read,
                        MTLRenderStages::Vertex | MTLRenderStages::Fragment,
                    );
                }
                let solid = match &f.occlusion {
                    Some(occlusion) => occlusion.from..f.passes[0].end,
                    None => f.passes[0].clone(),
                };
                // At most two (pipeline, command range) pairs; empty ranges skip.
                let draws: [(usize, std::ops::Range<usize>); 2] = match self.stage {
                    1 => [(0, solid), (1, f.passes[1].clone())],
                    3 => [
                        (
                            0,
                            f.occlusion
                                .as_ref()
                                .map_or(0..0, |occlusion| occlusion.occluders.clone()),
                        ),
                        (0, 0..0),
                    ],
                    _ => [(2, f.passes[2].clone()), (2, 0..0)],
                };
                for (pass, span) in draws {
                    let span = &span;
                    if span.is_empty() {
                        continue;
                    }
                    render.setRenderPipelineState(&f.pipelines[pass]);
                    render.setDepthStencilState(Some(&f.depth[usize::from(pass == 2)]));
                    render.setCullMode(if pass == 2 {
                        MTLCullMode::None
                    } else {
                        MTLCullMode::Back
                    });
                    render.executeCommandsInBuffer_withRange(
                        &f.slot.commands,
                        NSRange::new(span.start, span.len()),
                    );
                }
            }
        }
        Ok(())
    }
}
/// Metal's indirect-command optimization over each nonempty range, each its own
/// region (executeCommands may only start at a region's beginning).
///
/// # Safety
/// `command` is the open deferred command buffer, outside any pass, and the
/// ranges lie within the snapshot's command slot.
unsafe fn optimize(
    command: &ProtocolObject<dyn MTLCommandBuffer>,
    f: &Snapshot,
    ranges: &[std::ops::Range<usize>],
) -> Result<(), wgpu::hal::DeviceError> {
    if !f.optimize || ranges.iter().all(|range| range.is_empty()) {
        return Ok(());
    }
    let blit = command
        .blitCommandEncoder()
        .ok_or(wgpu::hal::DeviceError::OutOfMemory)?;
    for range in ranges.iter().filter(|range| !range.is_empty()) {
        // SAFETY: the caller guarantees the range is inside the slot's commands.
        unsafe {
            blit.optimizeIndirectCommandBuffer_withRange(
                &f.slot.commands,
                NSRange::new(range.start, range.len()),
            );
        }
    }
    blit.endEncoding();
    Ok(())
}

/// Builds the depth pyramid from the occluders' depth, then drops every later
/// command whose box lies wholly behind it.
///
/// # Safety
/// `command` is the open deferred command buffer, outside any pass, after the
/// occluder pass has stored its depth. All resources live in the snapshot.
unsafe fn occlude(
    command: &ProtocolObject<dyn MTLCommandBuffer>,
    f: &Snapshot,
    occlusion: &OcclusionSnapshot,
) -> Result<(), wgpu::hal::DeviceError> {
    let compute = command
        .computeCommandEncoder()
        .ok_or(wgpu::hal::DeviceError::OutOfMemory)?;
    let pyramid = &occlusion.pyramid;
    let [seed, reduce, test] = &occlusion.pipelines;
    let groups = |width: u32, height: u32| {
        (
            MTLSize {
                width: width as usize,
                height: height as usize,
                depth: 1,
            },
            MTLSize {
                width: 16,
                height: 16,
                depth: 1,
            },
        )
    };
    // SAFETY: every bound texture/buffer is retained by the snapshot; the
    // encoder is serial, so each dispatch sees the previous one's writes.
    unsafe {
        compute.setComputePipelineState(seed);
        compute.setTexture_atIndex(Some(&occlusion.depth), 0);
        compute.setTexture_atIndex(Some(&pyramid.levels[0]), 1);
        let size = [
            occlusion.viewport[0],
            occlusion.viewport[1],
            pyramid.size[0],
            pyramid.size[1],
        ];
        compute.setBytes_length_atIndex(
            std::ptr::NonNull::from(&size).cast(),
            std::mem::size_of_val(&size),
            0,
        );
        let (grid, group) = groups(pyramid.size[0], pyramid.size[1]);
        compute.dispatchThreads_threadsPerThreadgroup(grid, group);
        compute.setComputePipelineState(reduce);
        for level in 1..pyramid.levels.len() {
            compute.setTexture_atIndex(Some(&pyramid.levels[level - 1]), 0);
            compute.setTexture_atIndex(Some(&pyramid.levels[level]), 1);
            let (grid, group) = groups(
                (pyramid.size[0] >> level).max(1),
                (pyramid.size[1] >> level).max(1),
            );
            compute.dispatchThreads_threadsPerThreadgroup(grid, group);
        }
        let tested = f.count.saturating_sub(occlusion.from);
        if tested > 0 {
            compute.setComputePipelineState(test);
            let data: [u32; 8] = [
                occlusion.from as u32,
                f.count as u32,
                0,
                0,
                (occlusion.viewport[0] as f32).to_bits(),
                (occlusion.viewport[1] as f32).to_bits(),
                (pyramid.levels.len() as f32).to_bits(),
                0,
            ];
            compute.setBytes_length_atIndex(
                std::ptr::NonNull::from(&data).cast(),
                std::mem::size_of_val(&data),
                0,
            );
            compute.setBuffer_offset_atIndex(Some(&f.inputs), f.offsets[0], 1);
            compute.setBuffer_offset_atIndex(Some(&occlusion.culled), 0, 2);
            compute.setBuffer_offset_atIndex(Some(&f.slot.container), 0, 3);
            compute.setBuffer_offset_atIndex(Some(&f.buffers[1]), 0, 4);
            compute.setTexture_atIndex(Some(&pyramid.texture), 0);
            compute.useResource_usage(
                ProtocolObject::from_ref(&*f.slot.commands),
                MTLResourceUsage::Write,
            );
            compute.dispatchThreads_threadsPerThreadgroup(
                MTLSize {
                    width: tested,
                    height: 1,
                    depth: 1,
                },
                MTLSize {
                    width: test.threadExecutionWidth().min(tested),
                    height: 1,
                    depth: 1,
                },
            );
        }
    }
    compute.endEncoding();
    Ok(())
}

fn pyramid(device: &ProtocolObject<dyn MTLDevice>, viewport: [u32; 2]) -> Result<Pyramid, String> {
    let size = viewport.map(|side| side.max(1).next_power_of_two());
    let count = 32 - size[0].max(size[1]).leading_zeros();
    // SAFETY: creates new private textures and views of them; nothing is shared yet.
    unsafe {
        let descriptor =
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::R32Float,
                size[0] as usize,
                size[1] as usize,
                true,
            );
        descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
        descriptor.setStorageMode(MTLStorageMode::Private);
        descriptor.setMipmapLevelCount(count as usize);
        let texture = device
            .newTextureWithDescriptor(&descriptor)
            .ok_or("Native depth pyramid allocation failed")?;
        let levels = (0..count as usize)
            .map(|level| {
                texture
                    .newTextureViewWithPixelFormat_textureType_levels_slices(
                        MTLPixelFormat::R32Float,
                        MTLTextureType::Type2D,
                        NSRange::new(level, 1),
                        NSRange::new(0, 1),
                    )
                    .ok_or_else(|| "Native depth pyramid view failed".to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Pyramid {
            size,
            texture,
            levels,
        })
    }
}

pub(super) fn record_standalone(
    encoder: &mut wgpu::CommandEncoder,
    command: Arc<dyn wgpu::wgc::command::NativeCommand>,
) {
    // SAFETY: only the owned wrapper calls this, outside any pass, with a hook
    // whose snapshot retains every resource it encodes until completion.
    unsafe {
        encoder.native_command(command);
    }
}

pub(super) fn record_render(
    pass: &mut wgpu::RenderPass<'_>,
    command: Arc<dyn wgpu::wgc::command::NativeCommand>,
) {
    // SAFETY: only the owned wrapper calls this, in its matching Metal pass.
    unsafe {
        pass.native_command(command);
    }
}
struct Native {
    /// Run Metal's indirect-command optimization after culling. On by default;
    /// `BOB_NATIVE_ICB_OPTIMIZE=0` skips that blit pass, for measurement only.
    optimize: bool,
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    compute: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    /// Depth-pyramid seed and reduction, then the occlusion test.
    occlusion: [Retained<ProtocolObject<dyn MTLComputePipelineState>>; 3],
    pyramid: Option<Arc<Pyramid>>,
    /// Counts are copied to the CPU only by completed-submission callbacks.
    completed_culled: Arc<AtomicU64>,
    arguments: Retained<ProtocolObject<dyn MTLArgumentEncoder>>,
    page_arguments: Retained<ProtocolObject<dyn MTLArgumentEncoder>>,
    page_staging: Buffer,
    page_count: usize,
    pipelines: [Pipeline; 3],
    depth: [Depth; 2],
    sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
    texture: Texture,
    texture_source: wgpu::Texture,
    material: Buffer,
    slots: Vec<FrameSlot>,
    indices: wgpu::Buffer,
    index_quads: u32,
    input_bytes: Vec<u8>,
    belt: wgpu::util::StagingBelt,
    data: Vec<DrawData>,
}
fn command_storage(
    device: &ProtocolObject<dyn MTLDevice>,
    arguments: &ProtocolObject<dyn MTLArgumentEncoder>,
    capacity: usize,
) -> Result<(Icb, Buffer), String> {
    let descriptor = MTLIndirectCommandBufferDescriptor::new();
    descriptor.setCommandTypes(MTLIndirectCommandType::DrawIndexed);
    descriptor.setInheritBuffers(false);
    descriptor.setMaxVertexBufferBindCount(3);
    descriptor.setMaxFragmentBufferBindCount(2);
    descriptor.setInheritPipelineState(true);
    // SAFETY: each GPU command sets the three vertex and two fragment bindings
    // allocated by this descriptor, while the parent supplies the pipeline.
    unsafe {
        let commands = device
            .newIndirectCommandBufferWithDescriptor_maxCommandCount_options(
                &descriptor,
                capacity,
                MTLResourceOptions::StorageModePrivate,
            )
            .ok_or("Native indirect commands unsupported")?;
        let buffer = device
            .newBufferWithLength_options(
                arguments.encodedLength(),
                MTLResourceOptions::StorageModeShared,
            )
            .ok_or("ICB argument allocation failed")?;
        arguments.setArgumentBuffer_offset(Some(&buffer), 0);
        arguments.setIndirectCommandBuffer_atIndex(Some(&commands), 0);
        Ok((commands, buffer))
    }
}

pub fn create(
    device: &wgpu::Device,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Result<Option<Box<dyn TerrainEncoder>>, String> {
    let format = match format {
        wgpu::TextureFormat::Bgra8Unorm => MTLPixelFormat::BGRA8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb => MTLPixelFormat::BGRA8Unorm_sRGB,
        wgpu::TextureFormat::Rgba8Unorm => MTLPixelFormat::RGBA8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb => MTLPixelFormat::RGBA8Unorm_sRGB,
        _ => return Ok(None),
    };
    // SAFETY: retain live device/texture handles, dropping each wgpu guard before
    // creating other resources. Native objects are owned by the returned backend.
    unsafe {
        let Some(hal) = device.as_hal::<Metal>() else {
            return Ok(None);
        };
        let metal = hal.raw_device().clone();
        drop(hal);
        if !metal.supportsFamily(MTLGPUFamily::Apple4) && !metal.supportsFamily(MTLGPUFamily::Mac2)
        {
            return Ok(None);
        }
        if metal.argumentBuffersSupport() != MTLArgumentBuffersTier::Tier2 {
            return Ok(None);
        }
        let texture_source = texture.clone();
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let texture = raw_view(&view)?;
        let library = metal
            .newLibraryWithSource_options_error(
                &NSString::from_str(include_str!("terrain.metal")),
                None,
            )
            .map_err(|e| e.to_string())?;
        let function = |name: &str| {
            library
                .newFunctionWithName(&NSString::from_str(name))
                .ok_or_else(|| format!("Missing native terrain function {name}"))
        };
        let vertex = function("terrain")?;
        let solid = function("solid")?;
        let cutout = function("cutout")?;
        let kernel = function("cull_and_encode")?;
        let compute = compute_pipeline(&metal, &kernel, &[0, 1, 2, 3, 4, 5, 6, 8])?;
        let occlusion = ["hiz_seed", "hiz_reduce", "occlude"].map(|name| {
            function(name).and_then(|function| {
                let immutable: &[usize] = match name {
                    "hiz_seed" => &[0],
                    "hiz_reduce" => &[],
                    // Buffer 2 is the writable atomic counter, never immutable.
                    _ => &[0, 1, 3, 4],
                };
                compute_pipeline(&metal, &function, immutable)
            })
        });
        let [seed, reduce, test] = occlusion;
        let occlusion = [seed?, reduce?, test?];
        let arguments = kernel.newArgumentEncoderWithBufferIndex(3);
        let page_arguments = kernel.newArgumentEncoderWithBufferIndex(4);
        let page_staging = metal
            .newBufferWithLength_options(
                page_arguments.encodedLength(),
                MTLResourceOptions::StorageModeShared,
            )
            .ok_or("Native page staging allocation failed")?;
        std::ptr::write_bytes(
            page_staging.contents().as_ptr().cast::<u8>(),
            0,
            page_arguments.encodedLength(),
        );
        page_arguments.setArgumentBuffer_offset(Some(&page_staging), 0);

        let pipeline = |fragment: &ProtocolObject<dyn MTLFunction>,
                        translucent: bool|
         -> Result<Pipeline, String> {
            let descriptor = MTLRenderPipelineDescriptor::new();
            descriptor.setVertexFunction(Some(&vertex));
            descriptor.setInputPrimitiveTopology(MTLPrimitiveTopologyClass::Triangle);
            // Match wgpu's read-only buffer contract. Uploads precede this pass;
            // neither terrain stage changes geometry, camera or material bytes.
            for index in 0..3 {
                descriptor
                    .vertexBuffers()
                    .objectAtIndexedSubscript(index)
                    .setMutability(MTLMutability::Immutable);
            }
            for index in 0..2 {
                descriptor
                    .fragmentBuffers()
                    .objectAtIndexedSubscript(index)
                    .setMutability(MTLMutability::Immutable);
            }
            let vertices = MTLVertexDescriptor::new();
            for (index, format, step) in [
                (0, MTLVertexFormat::UInt3, MTLVertexStepFunction::PerVertex),
                (
                    1,
                    MTLVertexFormat::Float3,
                    MTLVertexStepFunction::PerInstance,
                ),
            ] {
                let attribute = vertices.attributes().objectAtIndexedSubscript(index);
                attribute.setFormat(format);
                attribute.setBufferIndex(index);
                attribute.setOffset(0);
                let layout = vertices.layouts().objectAtIndexedSubscript(index);
                layout.setStride(12);
                layout.setStepFunction(step);
                layout.setStepRate(1);
            }
            descriptor.setVertexDescriptor(Some(&vertices));
            descriptor.setFragmentFunction(Some(fragment));
            descriptor.setSupportIndirectCommandBuffers(true);
            descriptor.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
            let color = descriptor.colorAttachments().objectAtIndexedSubscript(0);
            color.setPixelFormat(format);
            if translucent {
                color.setBlendingEnabled(true);
                color.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
                color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
                color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            }
            metal
                .newRenderPipelineStateWithDescriptor_error(&descriptor)
                .map_err(|e| e.to_string())
        };
        let pipelines = [
            pipeline(&solid, false)?,
            pipeline(&cutout, false)?,
            pipeline(&cutout, true)?,
        ];
        let depth_state = |write: bool| -> Result<Depth, String> {
            let descriptor = MTLDepthStencilDescriptor::new();
            descriptor.setDepthCompareFunction(MTLCompareFunction::GreaterEqual);
            descriptor.setDepthWriteEnabled(write);
            metal
                .newDepthStencilStateWithDescriptor(&descriptor)
                .ok_or_else(|| "Native terrain depth state failed".into())
        };
        let depth = [depth_state(true)?, depth_state(false)?];
        let sampler_descriptor = MTLSamplerDescriptor::new();
        sampler_descriptor.setSAddressMode(MTLSamplerAddressMode::Repeat);
        sampler_descriptor.setTAddressMode(MTLSamplerAddressMode::Repeat);
        sampler_descriptor.setMinFilter(MTLSamplerMinMagFilter::Nearest);
        sampler_descriptor.setMagFilter(MTLSamplerMinMagFilter::Nearest);
        sampler_descriptor.setMipFilter(MTLSamplerMipFilter::Linear);
        sampler_descriptor.setLodMaxClamp(32.0);
        sampler_descriptor.setSupportArgumentBuffers(true);
        let sampler = metal
            .newSamplerStateWithDescriptor(&sampler_descriptor)
            .ok_or("Native terrain sampler failed")?;
        let material_encoder = solid.newArgumentEncoderWithBufferIndex(0);
        let material = metal
            .newBufferWithLength_options(
                material_encoder.encodedLength(),
                MTLResourceOptions::StorageModeShared,
            )
            .ok_or("Native material allocation failed")?;
        material_encoder.setArgumentBuffer_offset(Some(&material), 0);
        material_encoder.setTexture_atIndex(Some(&texture), 0);
        material_encoder.setSamplerState_atIndex(Some(&sampler), 1);
        let slots = (0..3)
            .map(|_| {
                let (commands, container) = command_storage(&metal, &arguments, 1)?;
                Ok(FrameSlot {
                    inputs: gpu_buffer(device, "Native frame inputs", 65536),
                    raw: Arc::new(Slot {
                        culled: occlusion_counter(&metal)?,
                        commands,
                        container,
                    }),
                    capacity: 1,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(Box::new(Native {
            optimize: std::env::var_os("BOB_NATIVE_ICB_OPTIMIZE").is_none_or(|value| value != "0"),
            device: metal,
            compute,
            occlusion,
            pyramid: None,
            completed_culled: Arc::new(AtomicU64::new(0)),
            arguments,
            page_arguments,
            page_staging,
            page_count: 0,
            pipelines,
            depth,
            sampler,
            texture,
            texture_source,
            material,
            slots,
            indices: quad_index_buffer(device, 1),
            index_quads: 1,
            input_bytes: Vec::new(),
            belt: wgpu::util::StagingBelt::new(device.clone(), 1 << 20),
            data: Vec::new(),
        })))
    }
}

impl TerrainEncoder for Native {
    fn take_occluded(&mut self) -> u32 {
        self.completed_culled
            .swap(0, Ordering::AcqRel)
            .min(u64::from(u32::MAX)) as u32
    }

    fn encode(
        &mut self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        frame: &Frame<'_>,
    ) -> Result<Encoded, String> {
        let count = frame.draws.len();
        if count > u32::MAX as usize
            || frame.camera.size() < 96
            || frame.planes.iter().flatten().any(|v| !v.is_finite())
        {
            return Err("Invalid native terrain frame".into());
        }
        let mut cursor = 0;
        for span in &frame.passes {
            if span.start != cursor || span.end < span.start || span.end > count {
                return Err("Invalid native pass span".into());
            }
            cursor = span.end;
        }
        if cursor != count {
            return Err("Incomplete native pass spans".into());
        }
        if frame.occluders > frame.passes[0].len() {
            return Err("Native occluders exceed the solid commands".into());
        }
        self.data.clear();
        for draw in frame.draws {
            if draw.indices.end < draw.indices.start
                || !draw.indices.start.is_multiple_of(6)
                || !draw.indices.end.is_multiple_of(6)
                || draw.base_vertex < 0
                || u64::from(draw.indices.end / 6) * 4 + draw.base_vertex as u64
                    > draw.vertices.size() / 12
                || (u64::from(draw.instance) + 1) * 12 > frame.bases.size()
                || draw.low.iter().chain(&draw.high).any(|v| !v.is_finite())
                || (0..3).any(|axis| draw.low[axis] > draw.high[axis])
            {
                return Err("Invalid native terrain command".into());
            }
            self.data.push(DrawData {
                low: [draw.low[0], draw.low[1], draw.low[2], 0.0],
                high: [draw.high[0], draw.high[1], draw.high[2], 0.0],
                args: [
                    draw.indices.start,
                    draw.indices.end - draw.indices.start,
                    draw.base_vertex as u32,
                    draw.instance,
                ],
                page: [0; 4],
            });
        }
        device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| e.to_string())?;
        let available = |slots: &[FrameSlot]| {
            slots
                .iter()
                .position(|slot| Arc::strong_count(&slot.raw) == 1)
        };
        if available(&self.slots).is_none() {
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(5)),
                })
                .map_err(|e| e.to_string())?;
        }
        let index = available(&self.slots)
            .ok_or("Submit or discard earlier native frames before encoding more")?;
        let storage = &mut self.slots[index];
        if count > storage.capacity {
            let capacity = count
                .checked_next_power_of_two()
                .ok_or("Native command count overflow")?;
            let bytes = capacity as u64 * 64;
            if bytes > device.limits().max_buffer_size {
                return Err("Native command buffer exceeds device limit".into());
            }
            let (commands, container) = command_storage(&self.device, &self.arguments, capacity)?;
            storage.raw = Arc::new(Slot {
                culled: storage.raw.culled.clone(),
                commands,
                container,
            });
            storage.capacity = capacity;
        }
        let quads = frame
            .draws
            .iter()
            .map(|draw| draw.indices.end / 6)
            .max()
            .unwrap_or(0);
        if quads > self.index_quads {
            let limit = (device.limits().max_buffer_size / 24).min(u64::from(u32::MAX / 6)) as u32;
            if quads > limit {
                return Err("Native quad indices exceed device limits".into());
            }
            self.index_quads = quads
                .checked_next_power_of_two()
                .unwrap_or(limit)
                .min(limit);
            self.indices = quad_index_buffer(device, self.index_quads);
        }
        let indices = raw_buffer(&self.indices)?;
        let camera = raw_buffer(frame.camera)?;
        let bases = raw_buffer(frame.bases)?;
        // Sized from last frame's page count so neither list regrows mid-frame.
        let expected = self.page_count.max(4);
        let mut buffers = Vec::with_capacity(6 + expected);
        buffers.extend([
            indices.clone(),
            camera.clone(),
            bases.clone(),
            self.material.clone(),
            storage.raw.container.clone(),
        ]);
        let mut pages: Vec<(wgpu::Buffer, Buffer)> = Vec::with_capacity(expected);
        for (draw, record) in frame.draws.iter().zip(&mut self.data) {
            let index = if let Some(index) = pages
                .iter()
                .position(|(buffer, _)| *buffer == draw.vertices)
            {
                index
            } else {
                let raw = raw_buffer(&draw.vertices)?;
                buffers.push(raw.clone());
                pages.push((draw.vertices.clone(), raw));
                pages.len() - 1
            };
            record.page[0] = index as u32;
        }
        if pages.len() > MAX_PAGES {
            return Err("Native geometry exceeds the argument table page limit".into());
        }
        // Only this CPU staging table is mutated directly. Queue copies protect
        // earlier frames that are still reading the GPU table and its pointers.
        unsafe {
            for (index, (_, buffer)) in pages.iter().enumerate() {
                self.page_arguments
                    .setBuffer_offset_atIndex(Some(buffer), 0, index);
            }
            for index in pages.len()..self.page_count {
                self.page_arguments.setBuffer_offset_atIndex(None, 0, index);
            }
        }
        self.page_count = pages.len();
        let align = |value: usize| value.div_ceil(256) * 256;
        let record_bytes = bytemuck::cast_slice::<DrawData, u8>(&self.data);
        let record_offset = 256usize;
        let page_offset = align(record_offset + record_bytes.len());
        let needed = align(page_offset + self.page_arguments.encodedLength());
        if needed as u64 > device.limits().max_buffer_size {
            return Err("Native input upload exceeds device limit".into());
        }
        if needed as u64 > storage.inputs.size() {
            storage.inputs = gpu_buffer(
                device,
                "Native frame inputs",
                (needed.next_power_of_two() as u64).min(device.limits().max_buffer_size),
            );
        }
        self.input_bytes.resize(needed, 0);
        self.input_bytes[..96].copy_from_slice(bytemuck::bytes_of(&CullData {
            planes: *frame.planes,
            count: [count as u32, 0, 0, 0],
        }));
        self.input_bytes[record_offset..record_offset + record_bytes.len()]
            .copy_from_slice(record_bytes);
        // SAFETY: this CPU-only staging allocation has exactly encodedLength
        // initialized bytes; it is never read by the GPU directly.
        unsafe {
            let bytes = std::slice::from_raw_parts(
                self.page_staging.contents().as_ptr().cast::<u8>(),
                self.page_arguments.encodedLength(),
            );
            self.input_bytes[page_offset..page_offset + bytes.len()].copy_from_slice(bytes);
        }
        let inputs = raw_buffer(&storage.inputs)?;
        buffers.push(inputs.clone());
        let mut initialize = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Native attachment initialization"),
        });
        self.belt
            .write_buffer(
                &mut initialize,
                &storage.inputs,
                0,
                wgpu::BufferSize::new(needed as u64).expect("input header is nonempty"),
            )
            .copy_from_slice(&self.input_bytes);
        self.belt.finish_and_recall_on_submit(&initialize);
        // Native bindings bypass ordinary draw validation. Declare every wgpu
        // allocation read so submission still rejects mapped/wrong-device input
        // and retains its core resource through the queue fence.
        initialize.transition_resources(
            [
                wgpu::BufferTransition {
                    buffer: frame.camera,
                    state: wgpu::BufferUses::UNIFORM,
                },
                wgpu::BufferTransition {
                    buffer: frame.bases,
                    state: wgpu::BufferUses::VERTEX,
                },
                wgpu::BufferTransition {
                    buffer: &self.indices,
                    state: wgpu::BufferUses::INDEX,
                },
                wgpu::BufferTransition {
                    buffer: &storage.inputs,
                    state: wgpu::BufferUses::STORAGE_READ_ONLY,
                },
            ]
            .into_iter()
            .chain(pages.iter().map(|(buffer, _)| wgpu::BufferTransition {
                buffer,
                state: wgpu::BufferUses::VERTEX,
            })),
            std::iter::once(wgpu::TextureTransition {
                texture: &self.texture_source,
                selector: None,
                state: wgpu::TextureUses::RESOURCE,
            }),
        );
        // Occlusion needs occluders, something after them to test and a depth
        // attachment the kernels may read; otherwise one pass draws everything.
        let depth = raw_view(frame.depth)?;
        let from = frame.passes[0].start + frame.occluders;
        let occlusion = if frame.occluders > 0
            && from < count
            && depth.usage().contains(MTLTextureUsage::ShaderRead)
        {
            let viewport = [frame.size[0].max(1), frame.size[1].max(1)];
            let size = viewport.map(|side| side.next_power_of_two());
            let pyramid = match &self.pyramid {
                Some(pyramid) if pyramid.size == size => pyramid.clone(),
                // Frames still in flight keep the old pyramid alive.
                _ => Arc::new(pyramid(&self.device, viewport)?),
            };
            self.pyramid = Some(pyramid.clone());
            Some(OcclusionSnapshot {
                from,
                occluders: frame.passes[0].start..from,
                depth,
                pyramid,
                viewport,
                pipelines: self.occlusion.clone(),
                culled: self.slots[index].raw.culled.clone(),
            })
        } else {
            None
        };
        if occlusion.is_some() {
            let slot = self.slots[index].raw.clone();
            // SAFETY: slot selection required exclusive backend ownership. No
            // encoded work or completion callback can still use this counter.
            unsafe {
                slot.culled
                    .contents()
                    .as_ptr()
                    .cast::<u32>()
                    .write_volatile(0);
            }
            let completed = self.completed_culled.clone();
            initialize.on_submitted_work_done(move || {
                // SAFETY: this callback runs after this submission's GPU work.
                // Holding the slot prevents CPU reset/reuse until the read ends.
                let value = unsafe {
                    slot.culled
                        .contents()
                        .as_ptr()
                        .cast::<u32>()
                        .read_volatile()
                };
                completed.fetch_add(u64::from(value), Ordering::Release);
            });
        }
        let occluding = occlusion.is_some();
        let snapshot = Arc::new(Snapshot {
            inputs,
            slot: self.slots[index].raw.clone(),
            buffers,
            texture: self.texture.clone(),
            pipelines: self.pipelines.clone(),
            depth: self.depth.clone(),
            compute: self.compute.clone(),
            _sampler: self.sampler.clone(),
            pages: pages.into_iter().map(|(_, raw)| raw).collect(),
            offsets: [record_offset, page_offset],
            count,
            passes: frame.passes.clone(),
            optimize: self.optimize,
            occlusion,
        });
        // SAFETY: this standalone callback follows the input copies and precedes
        // the enclosing world pass. Core retains the snapshot until completion.
        unsafe {
            initialize.native_command(Arc::new(Hook {
                frame: snapshot.clone(),
                stage: 0,
            }));
        }
        let calls = frame.passes.iter().filter(|span| !span.is_empty()).count() as u32
            + u32::from(occluding);
        Ok(Encoded {
            encoder: initialize,
            occluding: occluding.then(|| {
                (
                    Arc::new(Hook {
                        frame: snapshot.clone(),
                        stage: 3,
                    }) as Arc<dyn wgpu::wgc::command::NativeCommand>,
                    Arc::new(Hook {
                        frame: snapshot.clone(),
                        stage: 4,
                    }) as Arc<dyn wgpu::wgc::command::NativeCommand>,
                )
            }),
            opaque: Arc::new(Hook {
                frame: snapshot.clone(),
                stage: 1,
            }),
            translucent: Arc::new(Hook {
                frame: snapshot,
                stage: 2,
            }),
            target: frame.target.clone(),
            depth: frame.depth.clone(),
            clear: frame.clear,
            size: frame.size,
            calls,
        })
    }
}

fn compute_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    function: &ProtocolObject<dyn MTLFunction>,
    immutable: &[usize],
) -> Result<Retained<ProtocolObject<dyn MTLComputePipelineState>>, String> {
    let descriptor = MTLComputePipelineDescriptor::new();
    descriptor.setComputeFunction(Some(function));
    // SAFETY: the listed buffer slots exist in these kernels and their bytes
    // are only read. ICB pointees and the separate atomic counter may be written.
    unsafe {
        for &index in immutable {
            descriptor
                .buffers()
                .objectAtIndexedSubscript(index)
                .setMutability(MTLMutability::Immutable);
        }
        device
            .newComputePipelineStateWithDescriptor_options_reflection_error(
                &descriptor,
                MTLPipelineOption::empty(),
                None,
            )
            .map_err(|error| error.to_string())
    }
}

fn occlusion_counter(device: &ProtocolObject<dyn MTLDevice>) -> Result<Buffer, String> {
    // SAFETY: the new four-byte shared buffer has no outstanding users yet.
    unsafe {
        let buffer = device
            .newBufferWithLength_options(4, MTLResourceOptions::StorageModeShared)
            .ok_or("Native occlusion counter allocation failed")?;
        buffer.contents().as_ptr().cast::<u32>().write(0);
        Ok(buffer)
    }
}

fn quad_index_buffer(device: &wgpu::Device, quads: u32) -> wgpu::Buffer {
    let indices = super::quad_indices(quads);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Native immutable quad indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    })
}

#[cfg(test)]
mod deferred_tests {
    use std::sync::{Arc, Mutex};
    #[test]
    #[ignore = "Requires native Metal; proves HIZ reduction clamps rectangular mip axes"]
    fn hiz_reduce_preserves_minimum_when_one_source_axis_is_one() -> Result<(), String> {
        use super::*;

        let device = MTLCreateSystemDefaultDevice().ok_or("Metal device unavailable")?;
        let queue = device.newCommandQueue().ok_or("Metal queue unavailable")?;
        let library = device
            .newLibraryWithSource_options_error(
                &NSString::from_str(include_str!("terrain.metal")),
                None,
            )
            .map_err(|error| error.to_string())?;
        let function = library
            .newFunctionWithName(&NSString::from_str("hiz_reduce"))
            .ok_or("Production hiz_reduce function unavailable")?;
        let pipeline = device
            .newComputePipelineStateWithFunction_error(&function)
            .map_err(|error| error.to_string())?;
        let region = |width, height| MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width,
                height,
                depth: 1,
            },
        };
        let texture = |width, height| {
            // SAFETY: both test cases use nonzero dimensions and a valid
            // single-sample, non-mipmapped R32Float 2D texture format.
            let descriptor = unsafe {
                MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                    MTLPixelFormat::R32Float,
                    width,
                    height,
                    false,
                )
            };
            descriptor.setStorageMode(MTLStorageMode::Shared);
            descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
            device
                .newTextureWithDescriptor(&descriptor)
                .ok_or("HIZ test texture unavailable")
        };
        // Positive, exactly representable depths distinguish the true minimum
        // from the zero returned by the old kernel's out-of-bounds reads. Put
        // the minimum in different source positions to exercise both operands.
        for (width, height, values) in [(1, 2, [0.75f32, 0.25]), (2, 1, [0.25f32, 0.75])] {
            let source = texture(width, height)?;
            let target = texture(1, 1)?;
            let command = queue
                .commandBuffer()
                .ok_or("Metal command buffer unavailable")?;
            let compute = command
                .computeCommandEncoder()
                .ok_or("Metal compute encoder unavailable")?;
            // SAFETY: the source contains exactly width*height R32Float texels;
            // upload finishes before submission. Retained Shared textures and
            // pipeline stay alive until waitUntilCompleted, and no CPU access
            // overlaps the GPU dispatch on this test's private queue.
            unsafe {
                source.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                    region(width, height),
                    0,
                    std::ptr::NonNull::from(&values).cast(),
                    width * std::mem::size_of::<f32>(),
                );
                compute.setComputePipelineState(&pipeline);
                compute.setTexture_atIndex(Some(&source), 0);
                compute.setTexture_atIndex(Some(&target), 1);
                let one = MTLSize {
                    width: 1,
                    height: 1,
                    depth: 1,
                };
                compute.dispatchThreads_threadsPerThreadgroup(one, one);
                compute.endEncoding();
                command.commit();
                command.waitUntilCompleted();
            }
            assert_eq!(
                command.status(),
                MTLCommandBufferStatus::Completed,
                "{width}x{height} reduction failed: {:?}",
                command.error()
            );
            let mut result = f32::NAN;
            // SAFETY: successful GPU completion establishes CPU ownership;
            // result has space for the single Shared R32Float target texel.
            unsafe {
                target.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::from(&mut result).cast(),
                    std::mem::size_of::<f32>(),
                    region(1, 1),
                    0,
                );
            }
            assert_eq!(result, values[0].min(values[1]), "{width}x{height} minimum");
        }
        Ok(())
    }

    #[derive(Debug)]
    struct Probe {
        order: Arc<Mutex<Vec<u8>>>,
        number: u8,
    }
    impl wgpu::wgc::command::NativeCommand for Probe {
        unsafe fn encode(
            &self,
            _: &mut dyn wgpu::hal::DynCommandEncoder,
        ) -> Result<(), wgpu::hal::DeviceError> {
            self.order.lock().unwrap().push(self.number);
            Ok(())
        }
    }
    #[test]
    fn deferred_hooks_preserve_order_and_retire_after_fence_or_abandonment() {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut encoder = device.create_command_encoder(&Default::default());
        let probe = Arc::new(Probe {
            order: order.clone(),
            number: 1,
        });
        let weak = Arc::downgrade(&probe);
        // SAFETY: this test hook changes no HAL state or resources.
        unsafe {
            encoder.native_command(probe);
        }
        assert!(order.lock().unwrap().is_empty(), "hook ran before finish");
        let finished = encoder.finish();
        assert_eq!(*order.lock().unwrap(), [1]);
        assert!(weak.upgrade().is_some(), "resource lost before submission");
        queue.submit([finished]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .unwrap();
        assert!(weak.upgrade().is_none(), "resource retained after fence");
        for finish in [false, true] {
            let mut encoder = device.create_command_encoder(&Default::default());
            let probe = Arc::new(Probe {
                order: order.clone(),
                number: 2,
            });
            let weak = Arc::downgrade(&probe);
            unsafe {
                encoder.native_command(probe);
            }
            if finish {
                drop(encoder.finish());
            } else {
                drop(encoder);
            }
            assert!(weak.upgrade().is_none(), "abandoned hook leaked");
        }
        assert_eq!(*order.lock().unwrap(), [1, 2]);
    }
    #[test]
    fn render_hooks_reset_deduplicated_pipeline_and_preserve_stream_order() {
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl("@vertex fn vs() -> @builtin(position) vec4f { return vec4f(0,0,0,1); } @fragment fn fs() -> @location(0) vec4f { return vec4f(1); }".into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let order = Arc::new(Mutex::new(Vec::new()));
        for restore in [true, false] {
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mut encoder = device.create_command_encoder(&Default::default());
            unsafe {
                encoder.native_command(Arc::new(Probe {
                    order: order.clone(),
                    number: 1,
                }));
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
                unsafe {
                    pass.native_command(Arc::new(Probe {
                        order: order.clone(),
                        number: 2,
                    }));
                }
                if restore {
                    pass.set_pipeline(&pipeline);
                }
                pass.draw(0..3, 0..1);
            }
            unsafe {
                encoder.native_command(Arc::new(Probe {
                    order: order.clone(),
                    number: 3,
                }));
            }
            assert!(order.lock().unwrap().is_empty());
            let finished = encoder.finish();
            let error = pollster::block_on(scope.pop());
            assert_eq!(
                error.is_none(),
                restore,
                "pipeline restore validation: {error:?}"
            );
            if restore {
                assert_eq!(*order.lock().unwrap(), [1, 2, 3]);
                queue.submit([finished]);
            }
            order.lock().unwrap().clear();
        }
    }
}
