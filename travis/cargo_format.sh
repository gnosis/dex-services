#!/bin/bash

set -e

cargo fmt
echo $TRAVIS_PULL_REQUEST_BRANCH

if [[ $(git diff --stat) != '' ]]; then
  echo 'Cargo format caused changes, pushing updated version'

  git config --global user.email "travis@travis-ci.org"
  git config --global user.name "Travis CI"

  git commit -am "Travis autoformatting in build: $TRAVIS_BUILD_NUMBER"

  git remote add upstream https://$GITHUB_GNOSIS_INFO_API_TOKEN@github.com/gnosis/dex-services.git
  git push upstream HEAD:$TRAVIS_PULL_REQUEST_BRANCH
else
  echo 'Cargo format was already clean'
fi