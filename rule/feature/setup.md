# Feature Overview

It describes the feature of setting up the system at the first time.

- At first, the frontend should call `{base_api_url}/v1/initialized` to check if the system has been initialized before showing each page. The api will return `200`, if it is initialized.
- If not, redirect to `{base_url}/setup` to let the user set up the system.

## Setup Page

There are three inputs in the page: user name, email and transport service type. The supported types will be return from `{base_api_url}/v1/admin/setup/transport-service-type`. 

Here are additional options for different types:

- **Telegram**: the option for `Telegram` is the token of a `Telegram Bot`. And the api for setting up `Telegram` is `{base_api_url}/v1/admin/setup/telegram`.

Currently, only `Telegram` is supported.
