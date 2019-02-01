FROM node:8.11-alpine

# Create app directory
WORKDIR /app

# Dependencies will 	Install app dependencies
COPY . .

RUN  apk add --update --no-cache --virtual build-dependencies git python

RUN cd dex-contracts

RUN rm -fr .git || :
# # Compile necesary contracts for app and cleanup unnecesary files
RUN npm install

# # Compile necesary contracts for app and cleanup unnecesary files
# truffle compile
RUN npx truffle migrate --reset --network developmentdocker