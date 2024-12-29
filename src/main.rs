use std::{fs, path::Path, time::Duration, sync::Arc, net::SocketAddr, error::Error, collections::HashMap};
use axum::{
    routing::get,
    Router,
    response::IntoResponse,
    extract::State,
    Json,
};
use tokio::time;
use serde::Serialize;
use parking_lot::RwLock;
use prometheus::{TextEncoder, Registry, Gauge, Encoder, Opts};

#[derive(Clone)]
struct AppState {
    temperatures: Arc<RwLock<HashMap<String, f64>>>,
    registry: Arc<Registry>,
    temperature_gauges: Arc<RwLock<HashMap<String, Gauge>>>,
}

#[derive(Serialize)]
struct Temperature {
    sensor_id: String,
    celsius: f64,
}

async fn temperatures_handler(State(state): State<AppState>) -> Json<Vec<Temperature>> {
    let temps = state.temperatures.read();
    let readings: Vec<Temperature> = temps
        .iter()
        .map(|(sensor_id, temp)| Temperature {
            sensor_id: sensor_id.clone(),
            celsius: *temp,
        })
        .collect();
    Json(readings)
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
    let temp_line = content
        .lines()
        .nth(1)
        .ok_or("Temperature data not found")?;
    
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
                let mut temps = state.temperatures.write();
                let mut gauges = state.temperature_gauges.write();
                let sensors = entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("28-")
                    });

                for sensor in sensors {
                    let sensor_name = sensor.file_name().to_string_lossy().into_owned();
                    match read_temperature(&sensor.path()) {
                        Ok(temp) => {
                            temps.insert(sensor_name.clone(), temp);
                            
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
                        Err(e) => eprintln!("Failed to read temperature from {}: {}", sensor_name, e),
                    }
                }
            }
            Err(e) => eprintln!("Failed to read devices directory: {}", e),
        }
        
        time::sleep(Duration::from_secs(60)).await;
    }
}

fn register_gauge(registry: &Registry, device: &str) -> Gauge {
    let opts = Opts::new(
        "temperature_celsius",
        "Temperature reading in degrees Celsius",
    )
    .const_label("sensor", device);
    let gauge = Gauge::with_opts(opts).unwrap();
    registry.register(Box::new(gauge.clone())).unwrap();
    gauge
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting temperature monitoring service");

    // Create a new registry
    let registry = Registry::new();

    let state = AppState {
        temperatures: Arc::new(RwLock::new(HashMap::new())),
        registry: Arc::new(registry.clone()),
        temperature_gauges: Arc::new(RwLock::new(HashMap::new())),
    };

    let devices_path = "/sys/bus/w1/devices";
    match fs::read_dir(devices_path) {
        Ok(devices) => {
            //println!("Found {} temperature devices", devices.count());
            for device in devices {
                let device_name = device.unwrap().file_name().to_string_lossy().into_owned();
                if device_name.starts_with("28-") {
                    let gauge = register_gauge(&registry, &device_name);
                    state.temperature_gauges.write().insert(device_name, gauge);
                }
            }
        }
        Err(e) => {
            println!("Failed to read devices directory: {}. Using mock data for testing.", e);
            // Add a mock device for testing
            let mock_device = "28-mock".to_string();
            let gauge = register_gauge(&registry, &mock_device);
            state.temperature_gauges.write().insert(mock_device, gauge);
        }
    }

    let app_state = state.clone();
    tokio::spawn(async move {
        update_temperatures(Path::new(devices_path), app_state).await;
    });

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/temperatures", get(temperatures_handler))
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
