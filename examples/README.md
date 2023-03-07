# Examples

## Local Federation

Creates two instances which run on localhost and federate with each other. This setup is ideal for quick development and well as automated tests. In this case both instances run in the same process and are controlled from the main function. 

In case of Lemmy we are using the same setup for continuous integration tests, only that multiple instances are started with a bash script as different threads, and controlled over the API.

Use one of the following commands to run the example with the specified web framework:

`cargo run --example local_federation axum`

`cargo run --example local_federation actix-web`

## Live Federation

A minimal application which can be deployed on a server and federate with other platforms such as Mastodon. For this it needs run at the root of a (sub)domain which is available over HTTPS. Edit `main.rs` to configure the server domain and your Fediverse handle.

Setup instructions:

- Deploy the project to a server. For this you can clone the git repository on the server and execute `cargo run --example live_federation`. Alternatively run `cargo build --example live_federation` and copy the binary at `target/debug/examples/live_federation` to the server.
- Create a TLS certificate. With Let's Encrypt certbot you can use a command like `certbot certonly --nginx -d 'example.com' -m '*your-email@domain.com*'` (replace with your actual domain and email).
- Setup a reverse proxy which handles TLS and passes requests to the example project. With nginx you can use the following basic config, again using your actual domain:
```
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name example.com;
    ssl_certificate /etc/letsencrypt/live/example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/example.com/privkey.pem;
    location / {
        proxy_pass "http://localhost:8003";
        proxy_set_header Host $host;
    }
}
```
- Test with `curl -H 'Accept: application/activity+json' https://example.com/alison | jq` and `curl -H 'Accept: application/activity+json' "https://example.com/.well-known/webfinger?resource=acct:alison@example.com" | jq` that the server is setup correctly and serving correct responses.
- Login to a Fediverse platform like Mastodon, and search for `@alison@example.com`, with the actual domain and username from your `main.rs`. If you send a message, it will automatically send a response.