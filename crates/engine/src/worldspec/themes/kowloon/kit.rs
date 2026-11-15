//! Independent named streams keep existing parts stable when optional detail is added.
use super::signs::panel::Settings;
use superspec_types::rng::SeededRng;
const DETAIL_SCALE: [f64; 3] = [0.4, 1., 1.4];
pub struct Kit {
    settings: Settings,
    detail: u8,
}
#[derive(Debug, PartialEq)]
pub struct Choice {
    pub planned: bool,
    pub shown: bool,
}
impl Kit {
    pub fn new(settings: Settings, detail: u8) -> Result<Self, String> {
        settings.weathering.validate()?;
        if detail > 2 {
            return Err("Procedural detail must be 0, 1 or 2.".into());
        }
        super::timber::Timber::new(settings.timber.clone())?;
        Ok(Self { settings, detail })
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn detail(&self) -> u8 {
        self.detail
    }
    pub fn rng(&self, key: &str) -> Result<SeededRng, String> {
        if key.len() > 4096 {
            return Err("Procedural stream key exceeds its bounded allowance.".into());
        }
        Ok(SeededRng::from_text(&format!(
            "{}/{key}",
            self.settings.weathering.seed
        )))
    }
    pub fn option(&self, key: &str, chance: f64) -> Result<Choice, String> {
        if key.len() > 4095 || !chance.is_finite() || !(0. ..=4.).contains(&chance) {
            return Err("Procedural choice requires bounded probability and identity.".into());
        }
        let roll = self.rng(&format!("{key}?"))?.next_f64();
        Ok(Choice {
            planned: roll < chance * DETAIL_SCALE[2],
            shown: roll < chance * DETAIL_SCALE[self.detail as usize],
        })
    }
    pub fn has(&self, key: &str, chance: f64) -> Result<bool, String> {
        Ok(self.option(key, chance)?.shown)
    }
}
