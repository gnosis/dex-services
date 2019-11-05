#!/bin/bash

set -euo pipefail

#if [ "$TRAVIS_PULL_REQUEST" == "false" ] && [ "$TRAVIS_BRANCH" == "master" ]; then
  sudo apt-get update && sudo apt-get install awscli && sudo apt-get install python3-pip && sudo pip3 install --upgrade awscli
  # Get login token and execute login
  $(aws ecr get-login --no-include-email --region $AWS_REGION)
  mkdir .ssh
  echo $GITLAB_PRIVATE_KEY > .ssh/id_rsa

  echo "Building latest image with solver...";
  docker-compose build --build-arg use_solver=1 stablex

  echo "Pushing image";
  docker push $REGISTRY_URI/stablex:latest

  echo "The image has been pushed";
  rm -rf .ssh
#fi