# Feature Overview

Register command maybe sent from different transport services. The format will be `/register <email> <username>`. The system will:

- Check if the email exists in the system
- If not, a `PendingUser` with a `CreateEndpointRequest` will be created. `email` and `username` will be saved in the `PendingUser`, and `transport` info will be saved in the `CreateEndpointRequest`.
