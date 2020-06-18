#!/bin/bash

set -euxo pipefail

image_tag=$1

echo "Docker login"
echo "$DOCKER_PASSWORD" | docker login -u "$DOCKER_USERNAME" --password-stdin;

echo "Building price estimator docker";
docker build --tag gnosispm/dex-price-estimator-rust:$image_tag -f price-estimator/docker/Dockerfile .

echo "Pushing price estimator docker"
docker push gnosispm/dex-price-estimator-rust:$image_tag;

echo "done"

if [ "$image_tag" == "master" ] && [ -n "$AUTODEPLOY_URL_PRICE_ESTIMATOR" ]; then
    # Notifying webhook
    curl -s  \
      --output /dev/null \
      --write-out "%{http_code}" \
      -H "Content-Type: application/json" \
      -X POST \
      -d '{"push_data": {"tag": "'$AUTODEPLOY_TAG_PRICE_ESTIMATOR'" }}' \
      $AUTODEPLOY_URL_PRICE_ESTIMATOR
fi

