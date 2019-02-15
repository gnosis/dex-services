FROM node:8.11-alpine

# Create app directory
WORKDIR /app/dex-contracts

#TO be modified, need to copy only some files.
COPY . .

# Dependencies will 	Install app dependencies
RUN apk add --update --no-cache --virtual build-dependencies git python make g++ ca-certificates
RUN npm install
RUN truffle migrate --reset