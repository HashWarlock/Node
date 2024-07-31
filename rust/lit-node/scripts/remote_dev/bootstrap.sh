#!/bin/bash

source ./scripts/remote_dev/vars.sh

for i in "${GUESTS_WITH_SM[@]}"
do
  IP_ADDRESS="${i%/*}"
  echo "Setting up $IP_ADDRESS"
  ./scripts/remote_dev/single_node_scripts/bootstrap_remote_host.sh "$IP_ADDRESS" &
done
