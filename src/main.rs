use std::{fs, path::Path, time::Duration, sync::Arc, net::SocketAddr};

use anyhow::{Context, Result};
use axum::{
    routing::get,
    Router,
};
use prometheus_client::{
    encoding::text::encode,
    metrics::{family::Family, gauge::Gauge},
    registry::Registry,
};
use tokio::time;
use tracing::{info, warn};

struct AppState {
    registry: Arc<Registry>,
    temperature_gauge: Family<Vec<(String, String)>, Gauge<i64>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            temperature_gauge: self.temperature_gauge.clone(),
        }
    }
}

async fn metrics_handler(state: axum::extract::State<AppState>) -> String {
    let mut buffer = String::new();
    encode(&mut buffer, &state.registry).unwrap();
    buffer
}

fn read_temperature(device_path: &Path) -> Result<f64> {
    let content = fs::read_to_string(device_path.join("w1_slave"))?;
    let temp_line = content
        .lines()
        .nth(1)
        .context("Temperature data not found")?;
    
    let temp_str = temp_line
        .split("t=")
        .nth(1)
        .context("Temperature value not found")?;
    
    let temp_raw: i32 = temp_str.parse()?;
    Ok(temp_raw as f64 / 1000.0)
}

async fn update_temperatures(state: AppState) {
    let devices_path = Path::new("/sys/bus/w1/devices");
    
    loop {
        if let Ok(entries) = fs::read_dir(devices_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("28-") {
                            match read_temperature(&path) {
                                Ok(temp) => {
                                    let labels = vec![
                                        ("sensor".to_string(), name.to_string()),
                                    ];
                                    // Convert to millicelsius for better precision
                                    let temp_milli = (temp * 1000.0) as i64;
                                    state.temperature_gauge.get_or_create(&labels).set(temp_milli);
                                    info!("Temperature for {}: {:.3}°C", name, temp);
                                }
                                Err(e) => warn!("Failed to read temperature from {}: {}", name, e),
                            }
                        }
                    }
                }
            }
        }
        
        time::sleep(Duration::from_secs(60)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting temperature monitoring service");

    let mut registry = Registry::default();
    let temperature_gauge = Family::<Vec<(String, String)>, Gauge<i64>>::default();
    
    registry.register(
        "temperature_millicelsius",
        "Temperature reading in 1/1000 degrees Celsius",
        temperature_gauge.clone(),
    );

    let registry = Arc::new(registry);
    let state = AppState {
        registry,
        temperature_gauge,
    };

    let app_state = state.clone();
    tokio::spawn(async move {
        update_temperatures(app_state).await;
    });

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:9091".parse()?;
    info!("Starting server on {}", addr);
    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service(),
    )
    .await?;

    Ok(())
}
