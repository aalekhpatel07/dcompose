# dcompose

## Installation

```sh
cargo install dcompose
```

## Usage

Composing buncha services from across Github repositories:

```sh
dcompose \
    --output docker-compose.yml \
    "Data4Democracy/docker-scaffolding+master:docker-compose.yml@mongo" \ # get the mongo service from Data4Democracy/docker-scaffolding
    "omnivore-app/omnivore+main:docker-compose.yml@redis,x-postgres"  # get the redis and x-postgres services from omnivore
```

This creates a `docker-compose.yml` file with the following contents:
```yml
version: '3'
services:
  redis:
    image: redis:7.2.4
    container_name: omnivore-redis
    ports:
    - 6379:6379
    healthcheck:
      test:
      - CMD
      - redis-cli
      - --raw
      - incr
      - ping
  mongo:
    build: docker/mongo
    image: mongo
```
