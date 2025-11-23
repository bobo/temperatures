Simple prometheus exporter for temperature sensors using w1-gpio and Airthings air quality sensors.

I got tired of owserver and homeassistant fighting. So decided to skip the homeassistant step and just use prometheus.

## Features

- **W1-GPIO Temperature Sensors**: Reads temperature data from 1-Wire temperature sensors (e.g., DS18B20)
- **Airthings Integration**: Fetches air quality data from Airthings devices via their Consumer API

## Configuration

### W1-GPIO Sensors
No configuration needed. The exporter automatically detects sensors in `/sys/bus/w1/devices/`.

### Airthings Integration
To enable Airthings monitoring, you need to obtain API credentials:

1. Sign in to the [Airthings Dashboard](https://dashboard.airthings.com/)
2. Navigate to Integrations > API
3. Create a new API client and note your `Client ID` and `Client Secret`
4. Set the following environment variables:
   ```bash
   export AIRTHINGS_CLIENT_ID="your_client_id"
   export AIRTHINGS_CLIENT_SECRET="your_client_secret"
   ```

If these environment variables are not set, the exporter will skip Airthings monitoring and only export w1-gpio sensor data.

## Metrics

The exporter provides the following metrics at `http://localhost:9091/metrics`:

### W1-GPIO Temperature Sensors
- `temperature_celsius{sensor="<sensor_id>"}` - Temperature in degrees Celsius

### Airthings Sensors
Metrics are dynamically created based on the sensors available for each device. Common metrics include:
- `airthings_temp{device_id="<id>", device_type="<type>", device_name="<name>"}` - Temperature
- `airthings_humidity{device_id="<id>", device_type="<type>", device_name="<name>"}` - Relative humidity
- `airthings_co2{device_id="<id>", device_type="<type>", device_name="<name>"}` - CO2 concentration
- `airthings_voc{device_id="<id>", device_type="<type>", device_name="<name>"}` - Volatile Organic Compounds
- `airthings_pressure{device_id="<id>", device_type="<type>", device_name="<name>"}` - Atmospheric pressure
- `airthings_radonShortTermAvg{device_id="<id>", device_type="<type>", device_name="<name>"}` - Radon short term average
- `airthings_pm1{device_id="<id>", device_type="<type>", device_name="<name>"}` - PM1 particulate matter
- `airthings_pm25{device_id="<id>", device_type="<type>", device_name="<name>"}` - PM2.5 particulate matter
- `airthings_battery_percent{device_id="<id>", device_type="<type>", device_name="<name>"}` - Battery level

Available sensors vary by device type. The metric names match the sensor types returned by the Airthings API. Each metric includes the device name for easy identification in Grafana and other monitoring tools.

## Running

```bash
cargo build --release
./target/release/temperatures
```

The server will start on port 9091.
