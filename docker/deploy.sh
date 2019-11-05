#!/bin/bash

set -euo pipefail

#if [ "$TRAVIS_PULL_REQUEST" == "false" ] && [ "$TRAVIS_BRANCH" == "master" ]; then
  sudo apt-get update && sudo apt-get install -y python-pip && sudo pip install awscli

  # Get login token and execute login
  $(aws ecr get-login --no-include-email --region $AWS_REGION)
  echo -e $GITLAB_PRIVATE_KEY > .ssh/id_rsa
  chmod 0500 .ssh/id_rsa

  echo "Building latest image with solver...";
  docker-compose build --build-arg use_solver=1 stablex
  docker tag stablex $REGISTRY_URI

  echo "Pushing image";
  docker push $REGISTRY_URI

  echo "The image has been pushed";
  rm -rf .ssh/*
#fi