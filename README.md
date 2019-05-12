# thespis_impl_remote
The reference implementation of the thespis remote actor model


## TODO

- implement clone for service map?
- max length of messages in codec
- test everything

- use futures 0.3 codecs instead of tokio
- Peer should probably be able to tell the remote which services it provides.
- we don't close the connection when errors happen in the spawned tasks in send_service and call_service in the macro... bad! It also won't emit events for them...bad again!
- client code for remote actors is not generic, it will only work on MultiServiceImpl
- remote should store and resend messages for call if we don't get an acknowledgement? If ever you receive twice, you should drop it? Does tcp not guarantee arrival here? What with connection loss? The concept is best efforts to deliver a message at most once.
- write benchmarks for remote actors
- remote Addr? if the actor is known compile time?

## Remote design
