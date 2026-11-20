//! Geolocation from EXIF, check-ins, metadata.

use serde::{Deserialize, Serialize};
use security_platform_core::error::Result;
use tracing::info;
use chrono::Utc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoResult {
    pub locations: Vec<Location>,
    pub heatmap_data: Vec<HeatmapPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub source: String,
    pub timestamp: Option<chrono::DateTime<Utc>>,
    pub accuracy_meters: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatmapPoint {
    pub lat: f64,
    pub lng: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoArgs {
    pub target: String,
    pub extract_exif: bool,
}

pub async fn run(args: GeoArgs) -> Result<GeoResult> {
    info!("Geolocation lookup for {}", args.target);
    Ok(GeoResult {
        locations: vec![],
        heatmap_data: vec![],
    })
}

pub fn tool_spec() -> serde_json::Value {
    serde_json::json!({
        "name": "geolocation",
        "module": "socmed",
        "description": "Extract geolocation from metadata",
        "parameters": {
            "type": "object",
            "properties": {
                "target": {"type": "string"},
                "extract_exif": {"type": "boolean"}
            },
            "required": ["target"]
        }
    })
}