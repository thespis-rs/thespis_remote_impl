use crate::{ *, import::* } ;


/// Type responsible for knowing how call and send messages to an actor based on an Any pointer
/// ot that actors recipient, and a ServiceID.
///
/// This is the part of the code that is necessarily in the client code, usually by using a macro,
/// because the types of services are not known to the actor implementation.
//
pub trait ServiceMap
{
	/// Send a message to a handler. This should take care of deserialization.
	//
	fn send_service( &self, peer_addr: Addr<Peer>, msg: WireFormat )

		-> Result< Return<'static, Result<(), ThesErr>>, ThesRemoteErr >;


	/// Call a Service.
	/// This should take care of deserialization. The return address is the address of the peer
	/// to which the serialized answer shall be send.
	//
	fn call_service
	(
		&self                   ,
		 msg      :  WireFormat ,
		 peer_addr:  Addr<Peer> ,

	) -> Result< Return<'static, Result<(), ThesRemoteErr>>, ThesRemoteErr >;


	/// Get a list of all services provided by this service map.
	//
	fn services( &self ) -> Vec<&'static ServiceID>;
}
