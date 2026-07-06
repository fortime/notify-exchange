#! /bin/bash

replace_path_in_web() {
    local base_path
    case "$NE_BASE_PATH" in
        "/")
            base_path=""
            ;;
        *)
            base_path=${NE_BASE_PATH}
            ;;
    esac
    source /home/app/env_web_build
    find /home/app/web -type f \( -name "*.js" -o -name "*.html" -o -name "*.css" \) \
        -exec sed -i -e "s@$VUE_APP_PUBLIC_PATH@$base_path@g" -e "s@$VUE_APP_API_BASE_PATH@$NE_API_BASE_PATH@g" {} \;
}

replace_path_in_nginx() {
    local base_path api_base_path
    case "$NE_BASE_PATH" in
        *"/")
            base_path=$NE_BASE_PATH
            ;;
        *)
            base_path=${NE_BASE_PATH}/
            ;;
    esac
    case "$NE_API_BASE_PATH" in
        *"/")
            api_base_path=$NE_API_BASE_PATH
            ;;
        *)
            api_base_path=${NE_API_BASE_PATH}/
            ;;
    esac

    if [ "$NE_FRONTEND_ONLY" = "true" ]
    then
        sed "s@NE_BASE_PATH@${base_path}@g" /home/app/conf/nginx/nginx-frontend.conf.template > /home/app/conf/nginx/nginx-frontend.conf
    else
        sed "s@NE_BASE_PATH@${base_path}@g" /home/app/conf/nginx/nginx-all.conf.template > /home/app/conf/nginx/nginx-all.conf
        sed -i "s@NE_API_BASE_PATH@${api_base_path}@g" /home/app/conf/nginx/nginx-all.conf
    fi
}

touch_db() {
    local db_url db_path db_dir
    db_url="$1"
    # If using SQLite, ensure the parent directory and file exist
    if [[ "$db_url" == sqlite://* ]]; then
        db_path="${db_url#sqlite://}"

        # Handle the case where the URL might be sqlite: (relative) or sqlite:/// (absolute)
        # The prefix removal might leave a / for absolute paths.

        db_dir=$(dirname "$db_path")

        if [ ! -d "$db_dir" ]; then
            echo "Creating missing database directory: $db_dir"
            mkdir -p "$db_dir"
        fi

        if [ ! -f "$db_path" ]; then
            echo "Creating missing database file: $db_path"
            touch "$db_path"
        fi
    fi
}

if [ ! -f "/home/app/initialized" ]
then
    replace_path_in_web
    replace_path_in_nginx
    touch /home/app/initialized
fi

# Start the server
echo "Starting Server..."

if [ "$NE_FRONTEND_ONLY" = "true" ]
then
    exec /usr/bin/supervisord -c /home/app/conf/supervisord/supervisord-frontend.conf
else
    touch_db "$NOTIFY_EXCHANGE_DB_URL"
    touch_db "$NOTIFY_EXCHANGE_MSG_DB_URL"
    touch_db "$NOTIFY_EXCHANGE_CACHE_DB_URL"

    # Run migrations
    echo "Running database migrations..."
    /usr/local/bin/migration up --database-url "$NOTIFY_EXCHANGE_DB_URL"
    /usr/local/bin/migration up --database-url "$NOTIFY_EXCHANGE_MSG_DB_URL"
    /usr/local/bin/migration up --database-url "$NOTIFY_EXCHANGE_CACHE_DB_URL"

    if [ -z "$NOTIFY_EXCHANGE_BASE_URL" ]
    then
        export NOTIFY_EXCHANGE_BASE_URL="${NE_PROTOCOL}${NE_DOMAIN}${NE_BASE_PATH}"
    fi
    if [ -z "$NOTIFY_EXCHANGE_BASE_API_URL" ]
    then
        export NOTIFY_EXCHANGE_BASE_API_URL="${NE_PROTOCOL}${NE_DOMAIN}${NE_API_BASE_PATH}"
    fi
    if [ -z "$NOTIFY_EXCHANGE_HTTP_AUTH_URL_PATH_PREFIX" ]
    then
        export NOTIFY_EXCHANGE_HTTP_AUTH_URL_PATH_PREFIX="${NE_API_BASE_PATH}"
    fi
    exec /usr/bin/supervisord -c /home/app/conf/supervisord/supervisord-all.conf
fi
