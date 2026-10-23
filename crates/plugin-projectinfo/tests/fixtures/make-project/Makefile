# Build helpers.
SHELL := /bin/bash
APP ?= acme
VERSION = $(shell git describe --tags)
CFLAGS += -O2
export PATH := ./bin:$(PATH)

.PHONY: help build test clean deploy lint

.DEFAULT_GOAL := help

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort

## Compile the app
build: deps
	go build -o bin/$(APP) ./cmd/$(APP)

test: build ## Run the tests
	go test ./...

lint:
	golangci-lint run

deps:
	go mod download

clean: ## Remove build output
	rm -rf bin/

deploy: build ## Ship to production
	./scripts/deploy.sh

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

bin/$(APP): build

build: CFLAGS += -g

define HELP_TEXT
notatarget: really
endef

docker-build docker-push: ## Build or push the image
	docker $(subst docker-,,$@) acme
