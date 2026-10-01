# Build:  docker build -f deploy/orbit-node/Dockerfile -t orbit-node .
# Run:    docker run -d --name orbit-node -p 7443:7443/udp \
#           -v orbit-node-data:/var/lib/orbit-node \
#           -v $PWD/orbit-node.toml:/etc/orbit-node/orbit-node.toml:ro orbit-node
FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p orbit-node -p orbit-cli

FROM debian:bookworm-slim
RUN useradd --system --home /var/lib/orbit-node --shell /usr/sbin/nologin orbit \
    && mkdir -p /var/lib/orbit-node /etc/orbit-node \
    && chown orbit:orbit /var/lib/orbit-node
COPY --from=build /src/target/release/orbit-node /src/target/release/orbit-cli /usr/local/bin/
USER orbit
VOLUME ["/var/lib/orbit-node"]
EXPOSE 7443/udp
ENTRYPOINT ["orbit-node", "run", "--config", "/etc/orbit-node/orbit-node.toml"]
