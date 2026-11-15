use serde::Serialize;
use superspec_types::diagnostic::Diagnostic;

#[derive(Clone, Debug, Serialize)]
pub struct Suppression {
    pub reason: String,
    pub stale: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub id: String,
    pub fingerprint: String,
    pub state: &'static str,
    #[serde(flatten)]
    pub diagnostic: Diagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppression: Option<Suppression>,
}
