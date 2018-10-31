Network stack:

## TcpServer
Listens and accepts connections on a TCP socket.
Hands off connected stream to a TcpSession.
Keeps a list of peers on the network with connection information.
If asked to establish a permanent connection to a peer it will
connect and reconnect as necessary.

## TcpSession
Sends and receives framed messages on a Tcp Socket.
Messages arrive in order.
The TcpSession will stop on error.
Any unsent or unreceived messages will be lost.
Passes received messages to the Link module.

## Link
Provides the ability to send unicast and broadcast messages.
Uses TCP to preserve message order within a TCP session.
A suffix of the TCP session may be dropped on connection loss
so delivery is not guaranteed.
Listens on a socket and receives messages.

