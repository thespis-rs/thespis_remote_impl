use super::import::*;

#[ cfg( feature = "remote" ) ]
use
{
	serde:: { Serialize, Deserialize },
};



#[ derive( Actor ) ] pub struct Sum( pub u64 );

#[cfg_attr(feature = "remote", derive(Serialize, Deserialize))] #[ derive( Debug ) ] pub struct Add( pub u64 );
#[cfg_attr(feature = "remote", derive(Serialize, Deserialize))] #[ derive( Debug ) ] pub struct Show;

impl Message for Add  { type Return = ();  }
impl Message for Show { type Return = u64; }



impl Handler< Add > for Sum
{
	fn handle( &mut self, msg: Add ) -> ReturnNoSend<()> { Box::pin( async move
	{
		trace!( "called sum with: {:?}", msg );

		self.0 += msg.0;

	})}
}



impl Handler< Show > for Sum
{
	fn handle( &mut self, _msg: Show ) -> ReturnNoSend<u64> { Box::pin( async move
	{
		trace!( "called sum with: Show" );

		self.0

	})}
}
