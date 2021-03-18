#!/bin/bash

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
cd $DIR
docker build . -t path_optimizer
id=$(docker create path_optimizer bash)
docker cp $id:/opt/target.tar ./target.tar
rm -rf dist && mkdir dist
mv target.tar dist
cd dist && tar -xf target.tar && rm target.tar