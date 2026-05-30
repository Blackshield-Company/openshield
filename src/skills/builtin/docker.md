---
name: docker
description: Docker and containerization best practices
triggers:
  - docker
  - container
  - dockerfile
  - compose
  - image
  - containerize
tags:
  - devops
  - containers
---

# Docker Best Practices

## Dockerfile
- Use multi-stage builds to minimize image size
- Pin base image versions: `FROM rust:1.85-slim`, not `FROM rust:latest`
- Combine RUN commands: `RUN apt-get update && apt-get install -y ... && rm -rf /var/lib/apt/lists/*`
- Use `.dockerignore` to exclude build artifacts and secrets
- Never bake secrets into images — use runtime env vars or secrets management

## Compose
- Use `docker compose` (v2) not `docker-compose` (v1, deprecated)
- Define healthchecks in compose files
- Use named volumes for persistent data
- Set resource limits: `deploy.resources.limits`

## Security
- Run as non-root user: `USER 1000`
- Use distroless or scratch images for production
- Scan images with `docker scan` or Trivy
- Read-only root filesystem: `read_only: true`
