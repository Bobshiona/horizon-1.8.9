//! `ItemStack.getDisplayName`, `getRarity` and `getTooltip` (without
//! advanced tooltips), and the durability `renderItemOverlayIntoGUI` shows.
use crate::{item_data::ITEMS, ui::Resources};
use horizon_protocol_47::nbt::{Item, Tag};

/// The registry row for an item: exact metadata first, then any.
fn row(id: i16, damage: i16) -> Option<&'static (i16, i8, &'static str, u16, u8)> {
    let mut any = None;
    for entry in ITEMS.iter().filter(|entry| entry.0 == id) {
        if i16::from(entry.1) == damage {
            return Some(entry);
        }
        if entry.1 < 0 {
            any = Some(entry);
        }
    }
    any
}

fn display(item: &Item) -> Option<&Tag> {
    item.nbt.as_ref()?.get("display")
}

/// `display.Name`, if set.
pub fn custom_name(item: &Item) -> Option<&str> {
    display(item)?.get("Name")?.as_str()
}

fn hide_flags(item: &Item) -> i32 {
    item.nbt
        .as_ref()
        .and_then(|tag| tag.get("HideFlags"))
        .and_then(Tag::as_int)
        .unwrap_or(0)
}

fn unbreakable(item: &Item) -> bool {
    item.nbt
        .as_ref()
        .and_then(|tag| tag.get("Unbreakable"))
        .and_then(Tag::as_int)
        .is_some_and(|value| value != 0)
}

/// `Item.isDamageable` and `getMaxDamage`: zero for items without durability
/// and for unbreakable stacks.
pub fn max_damage(item: &Item) -> u16 {
    if unbreakable(item) {
        return 0;
    }
    row(item.id, 0).map_or(0, |entry| entry.3)
}

/// `Potion.potionTypes` names by effect ID.
const EFFECTS: [&str; 24] = [
    "",
    "moveSpeed",
    "moveSlowdown",
    "digSpeed",
    "digSlowDown",
    "damageBoost",
    "heal",
    "harm",
    "jump",
    "confusion",
    "regeneration",
    "resistance",
    "fireResistance",
    "waterBreathing",
    "invisibility",
    "blindness",
    "nightVision",
    "hunger",
    "weakness",
    "poison",
    "wither",
    "healthBoost",
    "absorption",
    "saturation",
];

/// `Potion.isBadEffect`.
fn harmful(effect: usize) -> bool {
    matches!(effect, 2 | 4 | 7 | 9 | 15 | 17 | 18 | 19 | 20)
}

fn custom_effects(item: &Item) -> Vec<(usize, i32, i32)> {
    item.nbt
        .as_ref()
        .and_then(|tag| tag.get("CustomPotionEffects"))
        .map(Tag::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(|effect| {
            let id = effect.get("Id")?.as_int()?;
            let amplifier = effect.get("Amplifier").and_then(Tag::as_int).unwrap_or(0);
            let duration = effect.get("Duration").and_then(Tag::as_int).unwrap_or(0);
            usize::try_from(id)
                .ok()
                .filter(|&id| (1..EFFECTS.len()).contains(&id))
                .map(|id| (id, amplifier, duration))
        })
        .collect()
}

/// `Potion.liquidColor` by effect ID.
const LIQUID: [u32; 24] = [
    3_693_766, 8_171_462, 5_926_017, 14_270_531, 4_866_583, 9_643_043, 16_262_179, 4_393_481,
    7_889_559, 5_578_058, 13_458_603, 10_044_730, 14_981_690, 3_035_801, 8_356_754, 2_039_587,
    2_039_713, 5_797_459, 4_738_376, 5_149_489, 3_484_199, 16_284_963, 2_445_989, 16_262_179,
];

/// Brewed potion metadata's effect, from `PotionHelper`'s liquid bits.
fn brewed_effect(damage: i16) -> Option<usize> {
    Some(match damage & 15 {
        1 => 10,
        2 => 1,
        3 => 12,
        4 => 19,
        5 => 6,
        6 => 16,
        8 => 18,
        9 => 5,
        10 => 2,
        12 => 7,
        13 => 13,
        14 => 14,
        _ => return None,
    })
}

/// A potion's liquid tint: its first custom effect, else its brewed effect,
/// else water.
pub fn potion_color(item: &Item) -> u32 {
    custom_effects(item)
        .first()
        .map(|effect| effect.0)
        .or_else(|| brewed_effect(item.damage))
        .map_or(LIQUID[0], |effect| LIQUID[effect])
}

/// `ItemArmor.getColor` for leather: `display.color` or the default brown.
pub fn leather_color(item: &Item) -> u32 {
    display(item)
        .and_then(|display| display.get("color"))
        .and_then(Tag::as_int)
        .map_or(10_511_680, |color| color as u32 & 0x00FF_FFFF)
}

/// `ItemPotion.getItemStackDisplayName` for water bottles and custom
/// effects; other potions show their item name.
fn potion_name(item: &Item, res: &Resources) -> Option<String> {
    if item.id != 373 {
        return None;
    }
    if item.damage == 0 {
        return Some(res.translate("item.emptyPotion.name").trim().to_owned());
    }
    let splash = if item.damage & 16384 != 0 {
        format!("{} ", res.translate("potion.prefix.grenade").trim())
    } else {
        String::new()
    };
    let (effect, _, _) = *custom_effects(item).first()?;
    let key = format!("potion.{}.postfix", EFFECTS[effect]);
    Some(format!("{splash}{}", res.translate(&key).trim()))
}

/// `ItemStack.getDisplayName`.
pub fn display_name(item: &Item, res: &Resources) -> String {
    if let Some(name) = custom_name(item) {
        return name.to_owned();
    }
    if let Some(name) = potion_name(item, res) {
        return name;
    }
    match row(item.id, item.damage) {
        Some(entry) => res
            .translate(&format!("{}.name", entry.2))
            .trim()
            .to_owned(),
        None => format!("#{}", item.id),
    }
}

fn enchantments<'a>(item: &'a Item, list: &str) -> &'a [Tag] {
    item.nbt
        .as_ref()
        .and_then(|tag| tag.get(list))
        .map(Tag::as_list)
        .unwrap_or_default()
}

/// `EnumRarity` colour code: enchanted stacks are rare, golden apples rare
/// or epic, records rare, enchanted books with stored enchantments uncommon.
pub fn rarity(item: &Item) -> char {
    match item.id {
        322 if item.damage > 0 => 'd',
        322 | 2256..=2267 => 'b',
        403 if !enchantments(item, "StoredEnchantments").is_empty() => 'e',
        _ if !enchantments(item, "ench").is_empty() => 'b',
        _ => 'f',
    }
}

/// `Enchantment` translation keys by ID.
fn enchantment_key(id: i32) -> Option<&'static str> {
    Some(match id {
        0 => "enchantment.protect.all",
        1 => "enchantment.protect.fire",
        2 => "enchantment.protect.fall",
        3 => "enchantment.protect.explosion",
        4 => "enchantment.protect.projectile",
        5 => "enchantment.oxygen",
        6 => "enchantment.waterWorker",
        7 => "enchantment.thorns",
        8 => "enchantment.waterWalker",
        16 => "enchantment.damage.all",
        17 => "enchantment.damage.undead",
        18 => "enchantment.damage.arthropods",
        19 => "enchantment.knockback",
        20 => "enchantment.fire",
        21 => "enchantment.lootBonus",
        32 => "enchantment.digging",
        33 => "enchantment.untouching",
        34 => "enchantment.durability",
        35 => "enchantment.lootBonusDigger",
        48 => "enchantment.arrowDamage",
        49 => "enchantment.arrowKnockback",
        50 => "enchantment.arrowFire",
        51 => "enchantment.arrowInfinite",
        61 => "enchantment.lootBonusFishing",
        62 => "enchantment.fishingSpeed",
        _ => return None,
    })
}

fn enchantment_lines(list: &[Tag], res: &Resources, out: &mut Vec<String>) {
    for entry in list {
        let (Some(id), Some(level)) = (
            entry.get("id").and_then(Tag::as_int),
            entry.get("lvl").and_then(Tag::as_int),
        ) else {
            continue;
        };
        if let Some(key) = enchantment_key(id) {
            out.push(format!(
                "{} {}",
                res.translate(key),
                res.translate(&format!("enchantment.level.{level}"))
            ));
        }
    }
}

/// The default `generic.attackDamage` modifier of swords and tools: the
/// tool's base plus the material's `getDamageVsEntity`.
fn weapon_damage(id: i16) -> Option<f64> {
    let material = |wood, stone, iron, diamond, gold| match id {
        _ if id == wood || id == gold => Some(0.0),
        _ if id == stone => Some(1.0),
        _ if id == iron => Some(2.0),
        _ if id == diamond => Some(3.0),
        _ => None,
    };
    if let Some(bonus) = material(268, 272, 267, 276, 283) {
        return Some(4.0 + bonus);
    }
    if let Some(bonus) = material(271, 275, 258, 279, 286) {
        return Some(3.0 + bonus);
    }
    if let Some(bonus) = material(270, 274, 257, 278, 285) {
        return Some(2.0 + bonus);
    }
    material(269, 273, 256, 277, 284).map(|bonus| 1.0 + bonus)
}

/// `ItemStack.DECIMALFORMAT`, `#.##`.
fn decimal(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let text = format!("{rounded:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn modifier_line(key: &str, amount: f64, operation: i32, res: &Resources) -> Option<String> {
    let shown = if operation == 1 || operation == 2 {
        amount * 100.0
    } else {
        amount
    };
    let name = res.translate(&format!("attribute.name.{key}")).to_owned();
    let (colour, format, value) = if amount > 0.0 {
        ('9', format!("attribute.modifier.plus.{operation}"), shown)
    } else if amount < 0.0 {
        ('c', format!("attribute.modifier.take.{operation}"), -shown)
    } else {
        return None;
    };
    let template = res.translate(&format).to_owned();
    Some(format!(
        "§{colour}{}",
        fill(&template, &[&decimal(value), &name])
    ))
}

/// `String.format` for the `%s`/`%n$s`/`%%` patterns of language entries.
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut digits = String::new();
        while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
            digits.push(*d);
            chars.next();
        }
        let index = if !digits.is_empty() && chars.peek() == Some(&'$') {
            chars.next();
            digits.parse::<usize>().ok().map(|n| n.saturating_sub(1))
        } else {
            None
        };
        match chars.next() {
            Some('%') => out.push('%'),
            Some('s' | 'd') => {
                let index = index.unwrap_or_else(|| {
                    next += 1;
                    next - 1
                });
                out.push_str(args.get(index).copied().unwrap_or(""));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `GuiContainer.renderToolTip`'s lines: `getTooltip`, then the rarity
/// colour on the name and grey on every other line.
pub fn lines(item: &Item, res: &Resources) -> Vec<String> {
    let hide = hide_flags(item);
    let mut name = display_name(item, res);
    if custom_name(item).is_some() {
        name = format!("§o{name}");
    }
    let mut out = vec![format!("{name}§r")];
    if hide & 32 == 0 {
        if item.id == 403 {
            enchantment_lines(enchantments(item, "StoredEnchantments"), res, &mut out);
        }
        for (effect, amplifier, duration) in custom_effects(item) {
            let mut line = res
                .translate(&format!("potion.{}", EFFECTS[effect]))
                .trim()
                .to_owned();
            if amplifier > 0 {
                line = format!(
                    "{line} {}",
                    res.translate(&format!("potion.potency.{amplifier}")).trim()
                );
            }
            if duration > 20 {
                let seconds = duration / 20;
                line = format!("{line} ({}:{:02})", seconds / 60, seconds % 60);
            }
            out.push(if harmful(effect) {
                format!("§c{line}")
            } else {
                line
            });
        }
    }
    if hide & 1 == 0 {
        enchantment_lines(enchantments(item, "ench"), res, &mut out);
    }
    if let Some(display) = display(item) {
        if display.get("color").is_some() {
            out.push(format!("§o{}", res.translate("item.dyed")));
        }
        for lore in display.get("Lore").map(Tag::as_list).unwrap_or_default() {
            if let Some(line) = lore.as_str() {
                out.push(format!("§5§o{line}"));
            }
        }
    }
    if hide & 2 == 0 {
        let custom: Vec<String> = item
            .nbt
            .as_ref()
            .and_then(|tag| tag.get("AttributeModifiers"))
            .map(Tag::as_list)
            .unwrap_or_default()
            .iter()
            .filter_map(|modifier| {
                let key = modifier.get("AttributeName")?.as_str()?;
                let amount = match modifier.get("Amount")? {
                    Tag::Double(value) => *value,
                    Tag::Float(value) => f64::from(*value),
                    other => f64::from(other.as_int()?),
                };
                let operation = modifier.get("Operation").and_then(Tag::as_int).unwrap_or(0);
                modifier_line(key, amount, operation, res)
            })
            .collect();
        let has_custom = item
            .nbt
            .as_ref()
            .and_then(|tag| tag.get("AttributeModifiers"))
            .is_some();
        let lines = if has_custom {
            custom
        } else {
            weapon_damage(item.id)
                .map(|base| base + 1.25 * f64::from(item.enchantment(16)))
                .and_then(|amount| modifier_line("generic.attackDamage", amount, 0, res))
                .into_iter()
                .collect()
        };
        if !lines.is_empty() {
            out.push(String::new());
            out.extend(lines);
        }
    }
    if unbreakable(item) && hide & 4 == 0 {
        out.push(format!("§9{}", res.translate("item.unbreakable")));
    }
    let rarity = rarity(item);
    out.into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("§{rarity}{line}")
            } else {
                format!("§7{line}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rows_agree_with_the_inventory_stack_limits() {
        for &(id, meta, key, _, stack) in ITEMS {
            assert!(
                key.starts_with("item.") || key.starts_with("tile."),
                "{id}:{meta} {key}"
            );
            if meta < 0 {
                assert_eq!(crate::inventory::stack_limit(id), stack as i8, "item {id}");
            }
        }
        assert_eq!(row(35, 14).unwrap().2, "tile.cloth.red");
        assert_eq!(crate::inventory::stack_limit(2256), 1);
        assert_eq!(crate::inventory::stack_limit(332), 16);
        assert_eq!(row(276, 5).unwrap().3, 1561);
        assert_eq!(weapon_damage(276), Some(7.0));
        assert_eq!(weapon_damage(286), Some(3.0));
        assert_eq!(decimal(8.25), "8.25");
        assert_eq!(decimal(7.0), "7");
        assert_eq!(fill("+%s %s", &["7", "Attack Damage"]), "+7 Attack Damage");
        assert_eq!(fill("%2$s %1$s%%", &["a", "b"]), "b a%");
    }
}
