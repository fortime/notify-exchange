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

# Remove possible .env* files and generate path placeholder
RUN rm -f ".env"* \
        && PATH_HOLDER_ID=$(date|sha1sum|sed 's/^\(.\{10\}\).*/\1/') \
        && echo "VUE_APP_API_BASE_PATH=__NE_API_BASE_PATH_${PATH_HOLDER_ID}" > .env \
        && echo "VUE_APP_PUBLIC_PATH=__NE_BASE_PATH_${PATH_HOLDER_ID}" >> .env

# Build app for production with minification
RUN npm run build

FROM docker.io/library/rust:1.87.0 AS builder-backend

# Set the working directory inside the container
WORKDIR /usr/src/app

# Copy the Cargo.toml and Cargo.lock files
COPY ./Cargo.toml ./Cargo.lock ./

# Create an empty src directory to trick Cargo into thinking it's a valid Rust project
RUN mkdir -p src/bin/ \
        && echo "fn main() {}" > src/bin/server.rs \
        && echo "fn main() {}" > src/bin/util.rs \
        && mkdir src/bin/http_proxy \
        && echo "fn main() {}" > src/bin/http_proxy/main.rs

# Build the dependencies without the actual source code to cache dependencies separately
RUN cargo build --release

# Now copy the source code
COPY ./src ./src

# Build your application
RUN touch -a -m src/bin/server.rs \
        && touch -a -m src/bin/util.rs \
        && touch -a -m src/bin/http_proxy/main.rs \
        && cargo build --release

# Start a new stage to create a smaller image without unnecessary build dependencies
FROM debian:bookworm-slim

# Set the working directory
WORKDIR /usr/src/app

RUN mkdir bin

RUN apt-get update \
        && apt-get install -y --no-install-recommends libssl3 openssl ca-certificates nginx supervisor \
        && rm -rf /var/lib/apt/lists/*

# Copy the built binary from the previous stage
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-server ./bin
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-http-proxy ./bin
COPY --from=builder-backend /usr/src/app/target/release/notify-exchange-util ./bin
COPY --from=builder-frontend /usr/src/app/dist ./web
COPY --from=builder-frontend /usr/src/app/.env ./env_web_build

# Create /var/www/html and link /usr/src/app/web to /var/www/html/web
RUN mkdir -p /var/www/html && ln -s /usr/src/app/web /var/www/html/web

# Copy nginx and supervisord configurations
COPY ./docker/entrypoint.sh /usr/src/app/bin/entrypoint.sh
COPY ./docker/nginx-all.conf.template /etc/nginx/conf.d/nginx-all.conf.template
COPY ./docker/nginx-frontend.conf.template /etc/nginx/conf.d/nginx-frontend.conf.template
COPY ./docker/supervisord-all.conf /etc/supervisor/conf.d/supervisord-all.conf
COPY ./docker/supervisord-frontend.conf /etc/supervisor/conf.d/supervisord-frontend.conf

RUN chmod a+x /usr/src/app/bin/entrypoint.sh

# Command to run the application
ENTRYPOINT ["/usr/src/app/bin/entrypoint.sh"]

EXPOSE 80
ENV NE_BASE_PATH "/"
ENV NE_API_BASE_PATH "/api"
