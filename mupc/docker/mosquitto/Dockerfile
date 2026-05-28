FROM eclipse-mosquitto:2

COPY config/mosquitto.conf /mosquitto/config/mosquitto.conf

# 创建数据目录
RUN mkdir -p /mosquitto/data /mosquitto/log

EXPOSE 1883 8883

CMD ["mosquitto", "-c", "/mosquitto/config/mosquitto.conf"]
