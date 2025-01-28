# 🌡️ Tempular (IoT Experiment)

Tempular is a simple IoT experiment that uses data transmitted to RabbitMQ via MQTT to pass data to a analytics engine written in Rust.

## 📦 Dev Environment

- Run the docker-compose file with `docker-compose up -f dockrer/dev.docker-compose.yml up --build`

### Test commands to check data flow:

- Make sure `mosquitto` is installed on the target macchine and the server is running
- `for i in {1..100}; do mosquitto_pub -h localhost -p 1883 -t "sensors/batch" -m "{\"humidity\": $((RANDOM % 100)), \"temperature\": $((RANDOM % 30 + 15)), \"pressure\": $((RANDOM % 50 + 980))}" && sleep 0.000000001; done`
- **TBD**: Add more commands to test the data flow