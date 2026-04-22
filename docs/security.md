# Forge Security

## Source Code Privacy Risks
For the MVP, code relies on external Inference providers (e.g. OpenRouter). Sensitive proprietary code should not be uploaded unless you have a trusted business agreement with the underlying AI models, or until offline inference (Candle) is fully implemented.

## Basic Auth Token Caveats
Forge returns a one-time use UUID token meant to be consumed immediately via HTTPS Basic Auth. The provided `clone_example` places the token directly into the Git URL.

### Shell History Leakage
Executing `git clone https://user:TOKEN@forge.ai.com/...` leaks the token to your shell's history file. This is generally accepted because the token becomes invalid exactly after it is used once (or expires in 1 hour). To prevent this, users should configure local `.netrc` or Git credential managers.

## TLS Assumptions
Forge expects a reverse proxy to handle HTTPS. It binds to HTTP. Passing Basic Auth over unencrypted HTTP exposes tokens in plaintext. Always put Forge behind Nginx, Caddy, or a load balancer.

## In-Memory Storage Tradeoffs
Since everything is kept in `DashMap` RAM:
- It eliminates disk IO bottlenecks and state corruption.
- However, extremely large files, or sudden spikes in uploads before clones execute, could OOM the server. A moderate rate-limiting approach using IP Token Buckets limits abuse vectors.
