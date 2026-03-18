import UdpSocket from "react-native-udp";

const SSDP_ADDRESS = "239.255.255.250";
const SSDP_PORT = 1900;
const FLICKSYNC_SERVICE_TYPE = "urn:flicksync:service:StateSync:1";
const DISCOVERY_TIMEOUT_MS = 3000;

const MSEARCH = [
  "M-SEARCH * HTTP/1.1",
  `HOST: ${SSDP_ADDRESS}:${SSDP_PORT}`,
  'MAN: "ssdp:discover"',
  "MX: 3",
  `ST: ${FLICKSYNC_SERVICE_TYPE}`,
  "",
  "",
].join("\r\n");

function parseLocation(response: string): URL | null {
  for (let line of response.split("\r\n")) {
    let lower = line.toLowerCase();
    if (lower.startsWith("location:")) {
      try {
        return new URL(line.substring("location:".length).trim());
      } catch (e) {
        console.warn(`SSDP Location header was invalid`, e);
      }
    }
  }

  return null;
}

export function ssdpDiscover(): Promise<URL[]> {
  return new Promise((resolve) => {
    let locations = new Set<URL>();
    let socket = UdpSocket.createSocket({ type: "udp4" });

    let finish = () => {
      try {
        socket.close();
      } catch {
        // ignore
      }
      resolve(Array.from(locations));
    };

    let timer = setTimeout(finish, DISCOVERY_TIMEOUT_MS);

    socket.on("error", () => {
      clearTimeout(timer);
      finish();
    });

    socket.on("message", (msg: Uint8Array) => {
      let text = new TextDecoder("utf-8").decode(msg);
      let location = parseLocation(text);
      if (location) {
        locations.add(location);
      }
    });

    socket.bind(0, () => {
      let message = new TextEncoder().encode(MSEARCH);

      socket.send(message, 0, message.length, SSDP_PORT, SSDP_ADDRESS, (e) => {
        console.warn("Failed to send SSDP search request", e);
      });
    });
  });
}
