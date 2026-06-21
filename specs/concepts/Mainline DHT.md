# Mainline DHT

BitTorrent's **Mainline DHT** — a global, decentralised Kademlia distributed
hash table with millions of participating nodes, originally used by BitTorrent
clients to find peers for a torrent without a central tracker. It stores small
values keyed by 160-bit ids and is reachable by any node that can speak the
protocol.

[[pkarr]] publishes its signed records to the Mainline DHT (with an HTTP relay
fallback), which is why ar-edit can use it as a **serverless meeting point** for
pairing ([[SPEC-003-realtime-collaborative-editing#ADR-013]]): no rendezvous
server to operate, on infrastructure [[iroh]] already relies on for discovery.
Trade-offs (public IP exposure, enumerable phrase-derived keys, lookup latency
folded into [[SPEC-003-realtime-collaborative-editing#NFR-010]]) are analysed in
ADR-013.
