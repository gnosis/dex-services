FROM node:10.15-alpine

# Create app directory
WORKDIR /app/dex-contracts
# Don't copy git artifacts as they contain backpointers to the parent repo
COPY dex-contracts/[^.git]* ./
COPY docker/truffle/run.sh /app/run.sh

# Dependencies will Install app dependencies
RUN apk add --update --no-cache --virtual build-dependencies git python make g++ ca-certificates
RUN npm install
RUN npm run network-inject