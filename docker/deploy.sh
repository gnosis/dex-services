#!/bin/bash

set -euo pipefail

image_name=$1

sudo apt-get update && sudo apt-get install -y python-pip && sudo pip install awscli

# Get login token and execute login
$(aws ecr get-login --no-include-email --region $AWS_REGION)

echo "Tagging latest image with solver...";
docker build --tag $REGISTRY_URI:$image_name -f docker/rust/release/Dockerfile .

echo "Pushing image";
docker push $REGISTRY_URI:$image_name

echo "The image has been pushed";
rm -rf .ssh/*

if [ "$image_name" == "master" ] && [ -n "$AUTODEPLOY_URL" ] && [ -n "$AUTODEPLOY_TOKEN" ]; then
    # Notifying webhook
    curl -s  \
      --output /dev/null \
      --write-out "%{http_code}" \
      -H "Content-Type: application/json" \
      -X POST \
      -d '{"token": "'$AUTODEPLOY_TOKEN'", "push_data": {"tag": "'$AUTODEPLOY_TAG'" }}' \
      $AUTODEPLOY_URL
fi
