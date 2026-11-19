use horizon_assets_189::Assets;
use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let jar = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Expected owned 1.8.9 jar path")?,
    );
    let assets = Assets::open(&jar)?;
    let mut count = 0;
    for model in ["stone", "dirt", "oak_planks", "glass", "oak_stairs"] {
        let quads = assets.bake_block(model)?;
        for quad in &quads {
            let texture = assets.texture(&quad.material.sprite)?;
            if texture.width == 0 || texture.height == 0 {
                return Err("Empty texture".into());
            }
        }
        println!("model={model} quads={}", quads.len());
        count += quads.len();
    }
    println!(
        "ASSETS_189_PASS models={} verified_quads={count}",
        assets.model_count
    );
    Ok(())
}
