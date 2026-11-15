//! Seeded bilingual businesses. Vocabulary is shipped data, independent of any JS runtime.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};
use superspec_types::rng::SeededRng;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Sector {
    Medical,
    Food,
    Retail,
    Service,
    Industry,
}
#[derive(Clone, Deserialize)]
struct Phrase {
    zh: String,
    en: String,
}
#[derive(Clone, Deserialize)]
struct Trade {
    zh: String,
    en: String,
    sector: Sector,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct Words {
    shop_names: Vec<Phrase>,
    surnames: Vec<Phrase>,
    given_names: Vec<Phrase>,
    trades: Vec<Trade>,
    practitioner_titles: BTreeMap<String, Vec<Phrase>>,
    taglines: BTreeMap<Sector, Vec<String>>,
    suffixes: BTreeMap<String, String>,
    numerals: Vec<String>,
}
fn words() -> &'static Words {
    static WORDS: OnceLock<Words> = OnceLock::new();
    WORDS.get_or_init(|| {
        serde_json::from_str(include_str!("words.json")).expect("shipped business vocabulary")
    })
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Business {
    pub name: String,
    pub trade: String,
    pub english: String,
    pub tagline: String,
    pub phone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor: Option<usize>,
    pub sector: Sector,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Brief {
    pub sectors: Option<Vec<Sector>>,
    pub trade: Option<String>,
    pub floor: Option<usize>,
}
impl Business {
    pub fn validate(&self) -> Result<(), String> {
        if [
            &self.name,
            &self.trade,
            &self.english,
            &self.tagline,
            &self.phone,
        ]
        .iter()
        .any(|s| s.len() > 1024)
            || self.name.is_empty()
            || self.trade.is_empty()
            || self.floor.is_some_and(|v| v > 1000)
        {
            return Err("Business copy requires bounded nonempty name/trade and floor.".into());
        }
        Ok(())
    }
}
pub fn invent(rng: &mut SeededRng, brief: &Brief) -> Result<Business, String> {
    if brief.trade.as_ref().is_some_and(|s| s.len() > 1024)
        || brief.sectors.as_ref().is_some_and(|v| v.len() > 5)
        || brief.floor.is_some_and(|v| v > 1000)
    {
        return Err("Business brief exceeds its text or sector budget.".into());
    }
    let w = words();
    let named = brief
        .trade
        .as_ref()
        .and_then(|s| w.trades.iter().find(|t| &t.zh == s));
    let pool: Vec<_> = w
        .trades
        .iter()
        .filter(|t| brief.sectors.as_ref().is_none_or(|s| s.contains(&t.sector)))
        .collect();
    if named.is_none() && pool.is_empty() {
        return Err("Business brief has no eligible trade.".into());
    }
    let trade = named.unwrap_or_else(|| rng.pick(&pool));
    let phone = format!(
        "{}-{}",
        rng.pick(&["3", "3", "3", "K"]),
        rng.integer(200000, 989999)
    );
    let tagline = rng.pick(&w.taglines[&trade.sector]);
    if let Some(titles) = w
        .practitioner_titles
        .get(&trade.zh)
        .filter(|_| rng.chance(0.5))
    {
        let surname = rng.pick(&w.surnames);
        let given = rng.pick(&w.given_names);
        let title = rng.pick(titles);
        return Ok(Business {
            name: format!("{}{}", surname.zh, given.zh),
            trade: title.zh,
            english: format!("DR {} {}", surname.en, given.en),
            tagline,
            phone,
            floor: brief.floor,
            sector: trade.sector,
        });
    }
    let shop = rng.pick(&w.shop_names);
    Ok(Business {
        name: shop.zh,
        trade: trade.zh.clone(),
        english: format!("{} {}", shop.en, trade.en),
        tagline,
        phone,
        floor: brief.floor,
        sector: trade.sector,
    })
}
pub fn title(b: &Business) -> String {
    format!("{}{}", b.name, b.trade)
}
pub fn floor_line(b: &Business) -> Option<String> {
    let floor = b.floor?;
    let w = words();
    if floor == 0 {
        return None;
    }
    Some(format!("{}{}", w.numerals.get(floor)?, w.suffixes["floor"]))
}
pub fn phone_line(b: &Business) -> String {
    format!("{} {}", words().suffixes["phone"], b.phone)
}
pub fn company_title(b: &Business, rng: &mut SeededRng) -> String {
    let w = words();
    format!(
        "{}{}",
        title(b),
        rng.pick(&[
            "",
            "",
            "",
            &w.suffixes["company"],
            &w.suffixes["company"],
            &w.suffixes["branch"]
        ])
    )
}
pub fn way_up(b: &Business) -> Option<String> {
    Some(format!("{}{}", words().suffixes["up"], floor_line(b)?))
}
