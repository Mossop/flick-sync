import {
  ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
} from "react";
import { AppState } from "react-native";
import { useDispatch } from "react-redux";
import UdpSocket from "react-native-udp";
import { setDiscoveredServers } from "../components/Store";
import { EventEmitter } from "expo";

const SSDP_ADDRESS = "239.255.255.250";
const SSDP_PORT = 1900;
const FLICKSYNC_SERVICE_TYPE = "urn:flicksync:service:StateSync:1";
const SEARCH_INTERVAL_MS = 30_000;
const EXPIRY_INTERVAL_MS = 5_000;
const EXPIRY_MS = 2 * 60 * 1000;

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
        console.warn("SSDP Location header was invalid", e);
      }
    }
  }
  return null;
}

interface SsdpEvents {
  servers(servers: string[]): void;

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  [event: string]: (...args: any[]) => void;
}

class SsdpService extends EventEmitter<SsdpEvents> {
  private socket: ReturnType<typeof UdpSocket.createSocket>;
  private searchInterval: NodeJS.Timeout;
  private expiryInterval: NodeJS.Timeout;
  private serverMap: Map<string, number>;

  constructor() {
    super();

    this.serverMap = new Map();

    this.socket = UdpSocket.createSocket({ type: "udp4" });

    this.socket.on("error", (e: unknown) => {
      console.warn("SsdpService: socket error", e);
    });

    this.socket.on("message", (msg: Uint8Array) => this.onSocketMessage(msg));

    this.socket.bind(SSDP_PORT, () => {
      try {
        this.socket.addMembership(SSDP_ADDRESS);
      } catch (e) {
        console.warn("SsdpService: addMembership failed", e);
      }

      this.sendSearch();
    });

    this.searchInterval = setInterval(
      () => this.sendSearch(),
      SEARCH_INTERVAL_MS,
    );

    this.expiryInterval = setInterval(
      () => this.expireServers(),
      EXPIRY_INTERVAL_MS,
    );
  }

  onSocketMessage(msg: Uint8Array) {
    let text = new TextDecoder("utf-8").decode(msg);
    if (!text.includes(FLICKSYNC_SERVICE_TYPE)) {
      return;
    }

    let location = parseLocation(text);
    if (!location) {
      return;
    }

    let url = location.toString();
    let wasAbsent = !this.serverMap.has(url);
    this.serverMap.set(url, Date.now());

    if (wasAbsent) {
      console.log(`Found new remote store at ${url}`);
      this.emit("servers", [...this.serverMap.keys()]);
    } else {
      console.log(`Saw existing remote store at ${url}`);
    }
  }

  expireServers() {
    let now = Date.now();
    let changed = false;

    for (let [url, ts] of this.serverMap) {
      if (now - ts > EXPIRY_MS) {
        console.log(`Expired remote store at ${url}`);
        this.serverMap.delete(url);
        changed = true;
      }
    }

    if (changed) {
      this.emit("servers", [...this.serverMap.keys()]);
    }
  }

  sendSearch() {
    console.log("Sending new search request");
    let message = new TextEncoder().encode(MSEARCH);
    this.socket.send(
      message,
      0,
      message.length,
      SSDP_PORT,
      SSDP_ADDRESS,
      (e) => {
        if (e) {
          console.warn("SsdpService: failed to send M-SEARCH", e);
        }
      },
    );
  }

  destroy() {
    clearInterval(this.searchInterval);
    clearInterval(this.expiryInterval);

    try {
      this.socket.close();
    } catch {
      // ignore
    }
  }
}

interface SsdpContextValue {
  suspend: () => void;
  resume: () => void;
}

const SsdpContext = createContext<SsdpContextValue>({
  suspend: () => {},
  resume: () => {},
});

export function useSsdp(): SsdpContextValue {
  return useContext(SsdpContext);
}

export default function SsdpServiceProvider({
  children,
}: {
  children: ReactNode;
}) {
  let dispatch = useDispatch();
  let service = useRef<SsdpService | null>(null);

  let updateServers = useCallback(
    (servers: string[]) => {
      dispatch(setDiscoveredServers(servers));
    },
    [dispatch],
  );

  let resume = useCallback(() => {
    if (!service.current) {
      service.current = new SsdpService();
      service.current.addListener("servers", updateServers);
    }
  }, [updateServers]);

  let suspend = useCallback(() => {
    if (service.current) {
      service.current.removeListener("servers", updateServers);
      service.current.destroy();
      service.current = null;
    }
  }, [updateServers]);

  useEffect(() => {
    resume();

    let sub = AppState.addEventListener("change", (state) => {
      if (state == "active") {
        resume();
      } else {
        suspend();
      }
    });

    return () => {
      sub.remove();
      suspend();
    };
  }, [resume, suspend]);

  return (
    <SsdpContext.Provider value={{ suspend, resume }}>
      {children}
    </SsdpContext.Provider>
  );
}
