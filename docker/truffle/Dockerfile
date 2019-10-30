FROM node:10.15-alpine

# Create app directory
WORKDIR /app/dex-contracts
COPY dex-contracts ./
COPY docker/truffle/run.sh /app/run.sh
# Since we only copy the submodule, get rid of the parent repository information
RUN rm -rf .git

# Dependencies will Install app dependencies
RUN apk add --update --no-cache --virtual build-dependencies git python make g++ ca-certificates
RUN npm install --production
RUN npm run network-inject