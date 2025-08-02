# Notify Exchange

## Project Overview

This application provides the ability for exchanging notifications through different message applications. It is built with a Rust backend (Axum) and a Vue.js frontend.

At its core, the system allows:
- **Users:** To register and manage their accounts.
- **Topics:** To create channels for notifications.
- **Endpoints:** To define destinations for notifications (e.g., a Telegram chat, an HTTP webhook URL).
- **Subscriptions:** To connect topics to endpoints, so that messages sent to a topic are delivered to all subscribed endpoints.

## High-Level Goals

The main goal is to build a robust and extensible notification exchange system. This includes:
1.  **Completing the backend API:** Implementing all the necessary endpoints for managing users, topics, endpoints, and subscriptions.
2.  **Building a functional frontend:** Creating a user-friendly interface for interacting with the system.
3.  **Implementing the notification logic:** Ensuring that notifications are reliably delivered from topics to subscribed endpoints.
4.  **Writing comprehensive tests:** Ensuring the stability and correctness of the application.

## Common Rules

- **Primitive Enum**: All variants are returned as a integer, I need you to read all enums under `src/db/custom_type.rs` from backend to generate a mapping for frontend.
- **Api Route**: All admin apis are put under `/{version}/admin`. All webhook apis are put under `/{version}/webhook`. The `version` currently is `v1`.
- **Naming**: I prefer 'singular' over 'plural' in the naming of folder.

## What I Need From You

To help you finish this project, I need you to provide me with clear and specific tasks. For example:
- "Implement the API endpoint for creating a new topic."
- "Create a Vue component to display a list of topics."
- "Write unit tests for the `UserService`."
- "Refactor the `TelegramService` to improve error handling."

## What I Will Do

Based on your requests, I will:
- **Analyze the codebase:** To understand the context and existing patterns.
- **Formulate a plan:** I will propose a step-by-step plan to accomplish the task.
- **Implement the changes:** Once you approve the plan, I will write the code, following the project's conventions.
- **Verify the changes:** I will run tests and linters to ensure the quality of the code.
- **Commit the changes:** I will commit the changes with a clear and descriptive message.

I will operate in two modes:
- **Plan Mode:** To research and create implementation plans.
- **Implement Mode:** To execute the approved plans.

I am ready to assist you in building the Notify Exchange. Please provide me with your first task.

## Frontend

Based on your requests, I will:
- **Javascript Framework:** vue.
- **CSS Framework:** bootstrap.
