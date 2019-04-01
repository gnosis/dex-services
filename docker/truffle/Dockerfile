FROM node:10.15-alpine

# Create app directory
WORKDIR /app/dex-contracts
COPY dex-contracts docker/truffle/run.sh ./

# Dependencies will Install app dependencies
RUN apk add --update --no-cache --virtual build-dependencies git python make g++ ca-certificates
RUN npm install --production