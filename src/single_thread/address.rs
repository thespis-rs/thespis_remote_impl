use crate :: { import::*, single_thread::* };


pub struct Addr< A: Actor >
{
	mb: mpsc::UnboundedSender<Box<dyn Envelope<A>>>,
}

impl<A> Addr<A> where A: Actor + 'static
{
	// TODO: take a impl trait instead of a concrete type. This can be fixed once we
	// ditch channels or write some channels that implement sink.
	//
	pub fn new( mb: mpsc::UnboundedSender<Box<dyn Envelope<A>>> ) -> Self
	{
		Self{ mb }
	}
}

impl<A> Address<A> for Addr<A>

	where A: Actor + 'static,

{
	fn send<M>( &mut self, msg: M ) -> Pin<Box< dyn Future<Output=()> + '_>>

		where A: Handler< M >,
		      M: Message<Result = ()> + 'static,

	{
		async move
		{
			let envl: Box< dyn Envelope<A> >= Box::new( SendEnvelope::new( msg ) );

			await!( self.mb.send( envl ) ).expect( "Failed to send to Mailbox" );

		}.boxed()
	}



	fn call<M: Message + 'static>( &mut self, msg: M ) -> Pin<Box< dyn Future< Output = M::Result > + '_> >

		where A: Handler< M > ,

	{
		async move
		{
			let (ret_tx, ret_rx) = oneshot::channel::<M::Result>();

			let envl: Box< dyn Envelope<A> > = Box::new( CallEnvelope::new( msg, ret_tx ) );

			// trace!( "Sending envl to Mailbox" );

			await!( self.mb.send( envl ) ).expect( "Failed to send to Mailbox" );

			await!( ret_rx ).expect( "Failed to receive response in Addr.call" )

		}.boxed()
	}
}

