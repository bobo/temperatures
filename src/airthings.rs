use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::{Duration, SystemTime};

type BoxError = Box<dyn Error + Send + Sync>;

const TOKEN_URL: &str = "https://accounts-api.airthings.com/v1/token";
const API_BASE_URL: &str = "https://consumer-api.airthings.com/v1";

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
struct AccountsResponse {
    accounts: Vec<AccountResponse>,
}

#[derive(Debug, Deserialize)]
struct AccountResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DevicesResponse {
    devices: Vec<Device>,
}

#[derive(Debug, Deserialize)]
pub struct Device {
    #[serde(rename = "serialNumber")]
    pub id: String,
    #[serde(rename = "type")]
    pub device_type: String,
    #[allow(dead_code)]
    pub sensors: Vec<String>,
    pub name: String,
    #[allow(dead_code)]
    pub home: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SensorsResponse {
    results: Vec<DeviceSensors>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSensors {
    pub serial_number: String,
    pub sensors: Vec<SensorResponse>,
    #[allow(dead_code)]
    pub battery_percentage: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensorResponse {
    pub sensor_type: String,
    pub value: f64,
    #[allow(dead_code)]
    pub unit: String,
}

pub struct AirthingsClient {
    config: AirthingsConfig,
    client: Client,
    access_token: Option<AccessToken>,
    account_id: Option<String>,
}

impl AirthingsClient {
    pub fn new(config: AirthingsConfig) -> Self {
        Self {
            config,
            client: Client::new(),
            access_token: None,
            account_id: None,
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

    async fn get_account_id(&mut self) -> Result<String, BoxError> {
        // Return cached account ID if available
        if let Some(account_id) = &self.account_id {
            return Ok(account_id.clone());
        }

        let token = self.get_access_token().await?;

        let response = self
            .client
            .get(format!("{}/accounts", API_BASE_URL))
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Failed to get accounts: {} - {}", status, text).into());
        }

        let text = response.text().await?;
        let accounts_response: AccountsResponse = serde_json::from_str(&text).map_err(|e| {
            format!(
                "Failed to parse accounts response: {}. Response body: {}",
                e, text
            )
        })?;

        let account_id = accounts_response
            .accounts
            .first()
            .ok_or("No accounts found")?
            .id
            .clone();

        self.account_id = Some(account_id.clone());
        Ok(account_id)
    }

    pub async fn get_devices(&mut self) -> Result<Vec<Device>, BoxError> {
        let account_id = self.get_account_id().await?;
        let token = self.get_access_token().await?;

        let response = self
            .client
            .get(format!("{}/accounts/{}/devices", API_BASE_URL, account_id))
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Failed to get devices: {} - {}", status, text).into());
        }

        let text = response.text().await?;
        let devices_response: DevicesResponse = serde_json::from_str(&text).map_err(|e| {
            format!(
                "Failed to parse devices response: {}. Response body: {}",
                e, text
            )
        })?;
        Ok(devices_response.devices)
    }

    pub async fn get_sensors(
        &mut self,
        serial_numbers: &[String],
    ) -> Result<Vec<DeviceSensors>, BoxError> {
        let account_id = self.get_account_id().await?;
        let token = self.get_access_token().await?;

        let mut url = format!("{}/accounts/{}/sensors", API_BASE_URL, account_id);

        // Add serial numbers as query parameters
        if !serial_numbers.is_empty() {
            url.push('?');
            for (i, sn) in serial_numbers.iter().enumerate() {
                if i > 0 {
                    url.push('&');
                }
                url.push_str(&format!("sn={}", sn));
            }
        }

        let response = self.client.get(&url).bearer_auth(&token).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Failed to get sensors: {} - {}", status, text).into());
        }

        let text = response.text().await?;
        let sensors_response: SensorsResponse = serde_json::from_str(&text).map_err(|e| {
            format!(
                "Failed to parse sensors response: {}. Response body: {}",
                e, text
            )
        })?;
        Ok(sensors_response.results)
    }
}
