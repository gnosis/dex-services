FROM node:10.15.1-alpine

# Create app directory
WORKDIR /app
COPY . .

WORKDIR /app/dex-contracts

# Dependencies will Install app dependencies
RUN apk add --update --no-cache --virtual build-dependencies git python make g++ ca-certificates
RUN npm install --production
RUN node_modules/.bin/truffle compile