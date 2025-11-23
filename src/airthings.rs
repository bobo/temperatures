use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::{Duration, SystemTime};

type BoxError = Box<dyn Error + Send + Sync>;

const TOKEN_URL: &str = "https://accounts-api.airthings.com/v1/token";
const API_BASE_URL: &str = "https://ext-api.airthings.com/v1";

#[derive(Debug, Clone)]
pub struct AirthingsConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug)]
struct AccessToken {
    token: String,
    expires_at: SystemTime,
}

#[derive(Debug, Deserialize)]
pub struct Device {
    pub id: String,
    #[serde(rename = "deviceType")]
    pub device_type: String,
    pub sensors: Vec<Sensor>,
}

#[derive(Debug, Deserialize)]
pub struct Sensor {
    pub id: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
}

#[derive(Debug, Deserialize)]
pub struct DeviceLatestSamples {
    pub data: SampleData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleData {
    pub battery: Option<f64>,
    pub co2: Option<f64>,
    pub humidity: Option<f64>,
    pub pm1: Option<f64>,
    pub pm25: Option<f64>,
    pub pressure: Option<f64>,
    pub radon_short_term_avg: Option<f64>,
    pub temp: Option<f64>,
    pub voc: Option<f64>,
}

pub struct AirthingsClient {
    config: AirthingsConfig,
    client: Client,
    access_token: Option<AccessToken>,
}

impl AirthingsClient {
    pub fn new(config: AirthingsConfig) -> Self {
        Self {
            config,
            client: Client::new(),
            access_token: None,
        }
    }

    async fn get_access_token(&mut self) -> Result<String, BoxError> {
        // Check if we have a valid token
        if let Some(token) = &self.access_token {
            if token.expires_at > SystemTime::now() {
                return Ok(token.token.clone());
            }
        }

        // Request new token using client credentials flow
        let params = [
            ("grant_type", "client_credentials"),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
            ("scope", "read:device:current_values"),
        ];

        let response = self.client.post(TOKEN_URL).form(&params).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Token request failed with status {}: {}", status, text).into());
        }

        let token_response: TokenResponse = response.json().await?;

        let expires_at = SystemTime::now() + Duration::from_secs(token_response.expires_in - 60);
        self.access_token = Some(AccessToken {
            token: token_response.access_token.clone(),
            expires_at,
        });

        Ok(token_response.access_token)
    }

    pub async fn get_devices(&mut self) -> Result<Vec<Device>, BoxError> {
        let token = self.get_access_token().await?;

        let response = self
            .client
            .get(format!("{}/devices", API_BASE_URL))
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Failed to get devices: {} - {}", status, text).into());
        }

        let devices: Vec<Device> = response.json().await?;
        Ok(devices)
    }

    pub async fn get_latest_samples(
        &mut self,
        device_id: &str,
    ) -> Result<DeviceLatestSamples, BoxError> {
        let token = self.get_access_token().await?;

        let response = self
            .client
            .get(format!(
                "{}/devices/{}/latest-samples",
                API_BASE_URL, device_id
            ))
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!(
                "Failed to get latest samples for device {}: {} - {}",
                device_id, status, text
            )
            .into());
        }

        let samples: DeviceLatestSamples = response.json().await?;
        Ok(samples)
    }
}
