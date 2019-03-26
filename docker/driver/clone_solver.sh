#!/usr/bin/env sh

# Authorize SSH Host
mkdir -p /root/.ssh
chmod 0700 /root/.ssh && \
ssh-keyscan gitlab.com > /root/.ssh/known_hosts

# Copy SSH key
cp .ssh/id_rsa /root/.ssh/id_rsa
cp .ssh/id_rsa.pub /root/.ssh/id_rsa.pub

# Clone and install dependencies
git clone git@gitlab.com:twalth3r/batchauctions.git
cd batchauctions
pip install -r requirements.txt