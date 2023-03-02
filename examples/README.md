# Examples

## Local Federation

Creates two instances which run on localhost and federate with each other. This setup is ideal for quick development and well as automated tests. In this case both instances run in the same process and are controlled from the main function. 

In case of Lemmy we are using the same setup for continuous integration tests, only that multiple instances are started with a bash script as different threads, and controlled over the API.

Use one of the following commands to run the example with the specified web framework:

`cargo run --example local_federation axum`

`cargo run --example local_federation actix-web`
