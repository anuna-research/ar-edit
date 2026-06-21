# NodeTicket

An [[iroh]] addressing token that bundles everything a peer needs to dial
another node: the node's public-key identity plus connection hints (direct
socket addresses and/or a relay URL). Exchanging a NodeTicket lets one peer
establish a direct, authenticated connection to another.

In [[SPEC-003-realtime-collaborative-editing]] NodeTickets are exchanged through
the [[Rendezvous Server]] during pairing
([[SPEC-003-realtime-collaborative-editing#REQ-071]],
[[SPEC-003-realtime-collaborative-editing#CON-014]]): after the [[SPAKE2]]
handshake confirms a shared key, the peers swap tickets and hand off to a direct
[[iroh]] connection, after which the rendezvous is dropped. The ticket payload
is opaque to the rendezvous server.
