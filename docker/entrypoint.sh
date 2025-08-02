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
    source /usr/src/app/env_web_build
    find /usr/src/app/web -type f \( -name "*.js" -o -name "*.html" -o -name "*.css" \) \
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
        sed "s@NE_BASE_PATH@${base_path}@g" /etc/nginx/conf.d/nginx-frontend.conf.template > /etc/nginx/sites-enabled/default
    else
        sed "s@NE_BASE_PATH@${base_path}@g" /etc/nginx/conf.d/nginx-all.conf.template > /etc/nginx/sites-enabled/default
        sed -i "s@NE_API_BASE_PATH@${api_base_path}@g" /etc/nginx/sites-enabled/default
    fi
}

if [ ! -f "/usr/src/app/initialized" ]
then
    replace_path_in_web
    replace_path_in_nginx
    touch /usr/src/app/initialized
fi

if [ "$NE_FRONTEND_ONLY" = "true" ]
then
    exec /usr/bin/supervisord -c /etc/supervisor/conf.d/supervisord-frontend.conf
else
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
    exec /usr/bin/supervisord -c /etc/supervisor/conf.d/supervisord-all.conf
fi
