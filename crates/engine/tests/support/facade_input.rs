use serde::Deserialize;
use superspec_aml::rng::SeededRng;
use superspec_engine::worldspec::themes::kowloon::{
    facade_cells::Role,
    facade_plan::Request,
    massing::{Lot, Rect},
    palette,
};
#[derive(Deserialize)]
pub struct LotData {
    pub kind: String,
    pub ground: f64,
    pub roof: f64,
    pub top: f64,
    pub stories: usize,
    #[serde(rename = "storyHeight")]
    pub story_height: f64,
    pub key: String,
}
#[derive(Clone, Deserialize)]
pub struct Input {
    pub role: Role,
    pub key: String,
    pub width: f64,
    pub shift: f64,
    pub body_span: [f64; 2],
    pub ground: Vec<Rect>,
    pub body: Vec<Rect>,
    pub shops: f64,
}
impl Input {
    pub fn request<'a>(&'a self, lot: &'a Lot) -> Request<'a> {
        Request {
            lot,
            role: self.role,
            key: &self.key,
            width: self.width,
            shift: self.shift,
            body_span: self.body_span,
            ground: &self.ground,
            body: &self.body,
            shops: self.shops,
        }
    }
}
impl LotData {
    pub fn build(&self, style_seed: &str, width: f64) -> Lot {
        let l = self;
        Lot {
            index: 0,
            kind: if l.kind == "well" { "well" } else { "tenement" },
            x0: 0.,
            x1: width,
            stories: l.stories,
            ground: l.ground,
            story_height: l.story_height,
            roof: l.roof,
            top: l.top,
            front: 0.,
            back: 0.,
            style: palette::lot_style(&mut SeededRng::from_text(style_seed)),
            key: l.key.clone(),
        }
    }
}
