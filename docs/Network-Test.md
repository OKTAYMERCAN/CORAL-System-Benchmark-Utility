# Network Test

The network test is **not part of the default run**. A real network measurement needs two machines, and a test against the machine itself would not tell you anything about the network.

## Run a real test

On machine A (the server):

```
syscheck serve
```

On machine B (the client), using the address of machine A:

```
syscheck net --server 192.168.1.10
```

or as part of a normal run: `syscheck --net-server 192.168.1.10`. If the server uses another port, write `IP:PORT` (`syscheck serve --port 6000`, `--server 192.168.1.10:6000`). The default port is 5201; allow it in the firewall of machine A.

**The result is limited by the weaker of the two devices.** If one side has a slow CPU, a slow network card, an old cable or weak Wi-Fi, that part sets the limit and the other side cannot show what it could do. To test a NAS, run `serve` on a fast computer and the test on the NAS (or the other way round) and compare both directions. Use a wired connection for a meaningful result.

## What is measured

| Test | Details |
| --- | --- |
| TCP throughput, upload and download | With 1 stream and with 4 parallel streams |
| Both directions at once | Upload and download together |
| Round-trip time | 64-byte echo messages: median (p50), p99 and jitter |
| Message-size sweep | Throughput for different message sizes |
| Gateway | Round-trip time to your router's address, by TCP connection attempts (a refused connection still proves the round trip) |
| Link speed | The speed the network adapter reports, when the system tells it |

Each throughput measurement runs for 5 seconds (2 seconds with `--quick`).

## Loopback (this machine only)

`--net-loopback` starts a server inside the program and tests against it. This measures the speed of this machine's own TCP stack and memory, **not a network**. The report labels it "loopback" and gives it no score. It is useful only to see how fast the machine could drive a network if the network were not the limit.

## Security of the test server

`syscheck serve` has **no authentication and no encryption**. It limits a single test to 60 seconds, a message to 16 MiB and the number of simultaneous connections to 64, but anybody who can reach the port can use it to make your computer send and receive data. Run it only on networks you trust, only while testing, and stop it with Ctrl-C afterwards. Do not expose it to the internet.

## Reading the numbers

* Gigabit Ethernet gives about 940 Mbit/s of TCP throughput at best. 2.5 and 10 Gigabit Ethernet need a good cable and a fast CPU on both sides.
* Four streams are faster than one when the connection has some delay (Wi-Fi) or when one CPU core cannot fill the link.
* A high p99 round-trip time or high jitter means an unstable connection, which hurts game servers and remote desktops even when throughput is fine.
