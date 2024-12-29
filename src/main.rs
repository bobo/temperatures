use axum::{extract::State, response::IntoResponse, routing::get, Router};
use parking_lot::RwLock;
use prometheus::{Encoder, Gauge, Opts, Registry, TextEncoder};
use std::{
    collections::HashMap, error::Error, fs, net::SocketAddr, path::Path, sync::Arc, time::Duration,
};
use tokio::time;

#[derive(Clone)]
struct AppState {
    registry: Arc<Registry>,
    temperature_gauges: Arc<RwLock<HashMap<String, Gauge>>>,
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = state.registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

fn read_temperature(device_path: &Path) -> Result<f64, Box<dyn Error>> {
    let content = fs::read_to_string(device_path.join("w1_slave"))?;
    let temp_line = content.lines().nth(1).ok_or("Temperature data not found")?;

    let temp_str = temp_line
        .split("t=")
        .nth(1)
        .ok_or("Temperature value not found")?;

    let temp_raw: i32 = temp_str.parse()?;
    Ok(temp_raw as f64 / 1000.0)
}

async fn update_temperatures(devices_path: &Path, state: AppState) {
    loop {
        match fs::read_dir(devices_path) {
            Ok(entries) => {
                let mut gauges = state.temperature_gauges.write();
                let sensors = entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("28-"));

                for sensor in sensors {
                    let sensor_name = sensor.file_name().to_string_lossy().into_owned();
                    match read_temperature(&sensor.path()) {
                        Ok(temp) => {
                            // Get or create gauge for this sensor
                            let gauge = gauges.entry(sensor_name.clone()).or_insert_with(|| {
                                let opts = Opts::new(
                                    "temperature_celsius",
                                    "Temperature reading in degrees Celsius",
                                )
                                .const_label("sensor", &sensor_name);
                                let gauge = Gauge::with_opts(opts).unwrap();
                                state.registry.register(Box::new(gauge.clone())).unwrap();
                                gauge
                            });

                            gauge.set(temp);
                            println!("Temperature for {}: {:.3}°C", sensor_name, temp);
                        }
                        Err(e) => {
                            eprintln!("Failed to read temperature from {}: {}", sensor_name, e)
                        }
                    }
                }
            }
            Err(e) => eprintln!("Failed to read devices directory: {}", e),
        }

        time::sleep(Duration::from_secs(60)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting temperature monitoring service");

    let state = AppState {
        registry: Arc::new(Registry::new()),
        temperature_gauges: Arc::new(RwLock::new(HashMap::new())),
    };

    let devices_path = "/sys/bus/w1/devices";
    let app_state = state.clone();
    tokio::spawn(async move {
        update_temperatures(Path::new(devices_path), app_state).await;
    });

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 9091));
    println!("Starting server on {}", addr);
    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service(),
    )
    .await?;

    Ok(())
}
