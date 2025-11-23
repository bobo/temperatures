mod airthings;

use airthings::{AirthingsClient, AirthingsConfig};
use axum::{extract::State, response::IntoResponse, routing::get, Router};
use parking_lot::RwLock;
use prometheus::{Encoder, Gauge, Opts, Registry, TextEncoder};
use std::{
    collections::HashMap, env, error::Error, fs, net::SocketAddr, path::Path, sync::Arc,
    time::Duration,
};
use tokio::time;

#[derive(Clone)]
struct AppState {
    registry: Arc<Registry>,
    temperature_gauges: Arc<RwLock<HashMap<String, Gauge>>>,
    airthings_gauges: Arc<RwLock<HashMap<String, Gauge>>>,
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

async fn update_airthings(mut client: AirthingsClient, state: AppState) {
    loop {
        match client.get_devices().await {
            Ok(devices) => {
                for device in devices {
                    match client.get_latest_samples(&device.id).await {
                        Ok(samples) => {
                            let mut gauges = state.airthings_gauges.write();
                            let data = samples.data;

                            // Helper macro to create/update gauge
                            macro_rules! update_gauge {
                                ($field:expr, $name:expr, $help:expr, $unit:expr) => {
                                    if let Some(value) = $field {
                                        let key = format!("{}_{}", device.id, $name);
                                        let gauge = gauges.entry(key).or_insert_with(|| {
                                            let opts = Opts::new($name, $help)
                                                .const_label("device_id", &device.id)
                                                .const_label("device_type", &device.device_type);
                                            let gauge = Gauge::with_opts(opts).unwrap();
                                            state.registry.register(Box::new(gauge.clone())).unwrap();
                                            gauge
                                        });
                                        gauge.set(value);
                                        println!(
                                            "{} for device {}: {:.2} {}",
                                            $name, device.id, value, $unit
                                        );
                                    }
                                };
                            }

                            update_gauge!(data.temp, "airthings_temperature_celsius", "Temperature reading in degrees Celsius", "°C");
                            update_gauge!(data.humidity, "airthings_humidity_percent", "Relative humidity percentage", "%");
                            update_gauge!(data.co2, "airthings_co2_ppm", "CO2 concentration in parts per million", "ppm");
                            update_gauge!(data.voc, "airthings_voc_ppb", "Volatile Organic Compounds in parts per billion", "ppb");
                            update_gauge!(data.pressure, "airthings_pressure_hpa", "Atmospheric pressure in hectopascals", "hPa");
                            update_gauge!(data.radon_short_term_avg, "airthings_radon_bqm3", "Radon short term average in Bq/m³", "Bq/m³");
                            update_gauge!(data.pm1, "airthings_pm1_ugm3", "PM1 particulate matter in µg/m³", "µg/m³");
                            update_gauge!(data.pm25, "airthings_pm25_ugm3", "PM2.5 particulate matter in µg/m³", "µg/m³");
                            update_gauge!(data.battery, "airthings_battery_percent", "Battery level percentage", "%");
                        }
                        Err(e) => eprintln!("Failed to get samples for device {}: {}", device.id, e),
                    }
                }
            }
            Err(e) => eprintln!("Failed to get Airthings devices: {}", e),
        }

        // Update every 5 minutes (Airthings data is updated every 5 minutes)
        time::sleep(Duration::from_secs(300)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting temperature monitoring service");

    let state = AppState {
        registry: Arc::new(Registry::new()),
        temperature_gauges: Arc::new(RwLock::new(HashMap::new())),
        airthings_gauges: Arc::new(RwLock::new(HashMap::new())),
    };

    // Spawn w1-gpio temperature sensor monitoring
    let devices_path = "/sys/bus/w1/devices";
    let app_state = state.clone();
    tokio::spawn(async move {
        update_temperatures(Path::new(devices_path), app_state).await;
    });

    // Spawn Airthings monitoring if credentials are provided
    if let (Ok(client_id), Ok(client_secret)) = (
        env::var("AIRTHINGS_CLIENT_ID"),
        env::var("AIRTHINGS_CLIENT_SECRET"),
    ) {
        println!("Airthings credentials found, starting Airthings monitoring");
        let config = AirthingsConfig {
            client_id,
            client_secret,
        };
        let client = AirthingsClient::new(config);
        let app_state = state.clone();
        tokio::spawn(async move {
            update_airthings(client, app_state).await;
        });
    } else {
        println!("Airthings credentials not found (AIRTHINGS_CLIENT_ID, AIRTHINGS_CLIENT_SECRET), skipping Airthings monitoring");
    }

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
