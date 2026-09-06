# mxroute

An async client for the [MXroute] email hosting API.

The API is a REST facade over DirectAdmin, so a client authenticates against one mail
server at a time and every request carries three headers rather than a single token.

This crate is under construction; the endpoint modules land in subsequent commits.

## License

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

[mxroute]: https://mxroute.com
