#!/usr/bin/env bash
set -e

echo "Building Forge..."
cargo build --release

echo "Setting up /opt/forge..."
sudo mkdir -p /opt/forge
sudo cp target/release/forge /opt/forge/
if [ ! -f /opt/forge/.env ]; then
    sudo cp .env.example /opt/forge/.env
    echo "Copied .env.example to /opt/forge/.env. Please edit it!"
fi

echo "Installing systemd service..."
sudo cp forge.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable forge
sudo systemctl restart forge

echo "Deployed successfully!"
