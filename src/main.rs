use std::{fs, path::Path, time::Duration, sync::Arc, net::SocketAddr, error::Error, collections::HashMap};
use axum::{
    routing::get,
    Router,
    Json,
    extract::State,
};
use tokio::time;
use serde::Serialize;
use parking_lot::RwLock;

#[derive(Clone)]
struct AppState {
    temperatures: Arc<RwLock<HashMap<String, f64>>>,
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

async fn update_temperatures(state: AppState) {
    let devices_path = Path::new("/sys/bus/w1/devices");
    
    loop {
        match fs::read_dir(devices_path) {
            Ok(entries) => {
                let mut temps = state.temperatures.write();
                let sensors = entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry.file_name()
                            .to_str()
                            .map(|name| name.starts_with("28-"))
                            .unwrap_or(false)
                    });

                for sensor in sensors {
                    let sensor_name = sensor.file_name().to_string_lossy().into_owned();
                    match read_temperature(&sensor.path()) {
                        Ok(temp) => {
                            temps.insert(sensor_name.clone(), temp);
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting temperature monitoring service");

    let state = AppState {
        temperatures: Arc::new(RwLock::new(HashMap::new())),
    };

    let app_state = state.clone();
    tokio::spawn(async move {
        update_temperatures(app_state).await;
    });

    let app = Router::new()
        .route("/temperatures", get(temperatures_handler))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:9091".parse()?;
    println!("Starting server on {}", addr);
    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service(),
    )
    .await?;

    Ok(())
}
