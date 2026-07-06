FROM docker.io/library/node:lts-alpine AS builder-frontend

# Set the working directory inside the container
WORKDIR /usr/src/app

# Copy both 'package.json' and 'package-lock.json' (if available)
COPY ./web/package*.json ./

# Install project dependencies
RUN npm install

# Copy project files and folders to the current working directory (i.e. 'app' folder)
COPY ./web/public ./public
COPY ./web/src ./src
COPY ./web/vue.config.js ./
COPY ./web/.eslintrc.js ./

# Remove possible .env* files and generate path placeholder
RUN rm -f ".env"* \
        && PATH_HOLDER_ID=$(date|sha1sum|sed 's/^\(.\{10\}\).*/\1/') \
        && echo "VUE_APP_API_BASE_PATH=__NE_API_BASE_PATH_${PATH_HOLDER_ID}" > .env \
        && echo "VUE_APP_PUBLIC_PATH=__NE_BASE_PATH_${PATH_HOLDER_ID}" >> .env

# Build app for production with minification
RUN npm run build

FROM docker.io/library/rust:1.94.0 AS builder-backend

# Set the working directory inside the container
WORKDIR /usr/src/app

# Copy the Cargo.toml and Cargo.lock files
COPY ./Cargo.toml ./Cargo.lock ./

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p src/bin/ \
        && echo "fn main() {}" > src/bin/server.rs \
        && echo "fn main() {}" > src/bin/util.rs \
        && mkdir src/bin/http_proxy \
        && echo "fn main() {}" > src/bin/http_proxy/main.rs \
        && mkdir -p migration/src \
        && touch migration/src/lib.rs \
        && echo "fn main() {}" > migration/src/main.rs

COPY ./migration/Cargo.toml ./migration/Cargo.lock ./migration/

# Build the dependencies without the actual source code to cache dependencies separately
RUN cargo build --release \
        && cd migration \
        && cargo build --release

# Now copy the source code
COPY ./src ./src
COPY ./migration/src ./migration/src

# Build your application
RUN touch -a -m src/bin/server.rs \
        && touch -a -m src/bin/util.rs \
        && touch -a -m src/bin/http_proxy/main.rs \
        && cargo build --release \
        && cd migration \
        && touch -a -m src/main.rs \
        && touch -a -m src/lib.rs \
        && cargo build --release

# Start a new stage to create a smaller image without unnecessary build dependencies
FROM debian:bookworm-slim

RUN apt-get update \
        && apt-get install -y --no-install-recommends libssl3 openssl ca-certificates nginx supervisor \
        && rm -rf /var/lib/apt/lists/*

# Create a non-root user for security
RUN useradd -m app

# Copy the built binary from the previous stage
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-server /usr/local/bin/notify-exchange-server
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-http-proxy /usr/local/bin/notify-exchange-http-proxy
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-util /usr/local/bin/notify-exchange-util
COPY --from=builder-backend /usr/src/app/migration/target/release/migration /usr/local/bin/migration

# Copy nginx and supervisord configurations
COPY ./docker/entrypoint.sh /usr/local/bin/entrypoint.sh

# Create /var/www/html and link /home/app/web to /var/www/html/web
RUN mkdir -p /var/www/html \
        && ln -s /home/app/web /var/www/html/web \
        && chmod a+x /usr/local/bin/entrypoint.sh

COPY --from=builder-frontend /usr/src/app/dist /home/app/web
RUN chown -R app:app /home/app/web

USER app
WORKDIR /home/app

COPY --from=builder-frontend /usr/src/app/.env /home/app/env_web_build

RUN mkdir -p /home/app/data \
        && mkdir -p /home/app/conf/nginx \
        && mkdir -p /home/app/conf/supervisord \
        && mkdir -p /home/app/run/nginx

COPY ./docker/nginx-all.conf.template /home/app/conf/nginx/nginx-all.conf.template
COPY ./docker/nginx-frontend.conf.template /home/app/conf/nginx/nginx-frontend.conf.template
COPY ./docker/supervisord-all.conf /home/app/conf/supervisord/supervisord-all.conf
COPY ./docker/supervisord-frontend.conf /home/app/conf/supervisord/supervisord-frontend.conf

EXPOSE 3000

ENV NE_BASE_PATH "/"
ENV NE_API_BASE_PATH "/api"
ENV NOTIFY_EXCHANGE_DB_URL="sqlite:///home/app/data/ne.db"
ENV NOTIFY_EXCHANGE_MSG_DB_URL="sqlite:///home/app/data/ne-msg.db"
ENV NOTIFY_EXCHANGE_CACHE_DB_URL="sqlite:///home/app/data/ne-cache.db"
ENV NOTIFY_EXCHANGE_REFRESHING_DISPATCHERS_AT=05:00:00
ENV RUST_LOG=info

# Command to run the application
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
