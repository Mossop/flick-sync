import { LitElement, html, css, nothing, ref, keyed, classMap } from "lit";
import shoelaceStyles from "@shoelace/themes/dark.styles.js";
import "@shoelace/components/icon-button/icon-button.js";
import "@shoelace/components/dropdown/dropdown.js";

function pad(val) {
  if (val > 9) {
    return val.toString();
  }

  return `0${val}`;
}

function formatTime(time) {
  time = Math.round(time);
  let seconds = time % 60;
  time = (time - seconds) / 60;
  let minutes = time % 60;
  let hours = (time - minutes) / 60;

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  } else {
    return `${pad(minutes)}:${pad(seconds)}`;
  }
}

export class VideoPlayer extends LitElement {
  static styles = [
    shoelaceStyles,
    css`
      :host {
        position: relative;
      }

      video {
        width: 100%;
        height: 100%;
        object-fit: contain;
        object-position: center center;
        background-color: black;
      }

      .overlay {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: stretch;

        opacity: 0;
        background-color: hsl(from var(--sl-color-neutral-0) h s l / 0);
        cursor: none;
        transition:
          opacity var(--sl-transition-slow) cubic-bezier(0.4, 0, 0.2, 1) 0ms,
          background-color var(--sl-transition-slow)
            cubic-bezier(0.4, 0, 0.2, 1) 0ms;

        &.visible,
        &.casting,
        &.paused {
          background-color: hsl(from var(--sl-color-neutral-0) h s l / 0.3);
          opacity: 1;
          cursor: auto;
        }
      }

      .info {
        position: absolute;
        top: var(--sl-spacing-large);
        left: var(--sl-spacing-large);

        img {
          max-height: 150px;
          max-width: 150px;
        }
      }

      .main {
        flex: 1;
        display: flex;
        flex-direction: row;
        align-items: center;
        justify-content: center;
        gap: var(--sl-spacing-large);
        font-size: 200%;

        sl-icon-button::part(base) {
          color: var(--sl-color-neutral-800);
        }

        sl-icon-button::part(base):hover {
          color: var(--sl-color-primary-800);
        }

        .togglePlayback {
          font-size: 300%;
        }
      }

      .controls {
        background: var(--sl-color-neutral-300);
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: var(--sl-spacing-x-small);

        --connected-color: var(--sl-color-primary-600);
        --disconnected-color: var(--sl-color-neutral-600);
      }

      sl-icon-button {
        font-size: 200%;
      }

      .time {
        width: 4.2em;

        &.start {
          text-align: end;
        }

        &.end {
          text-align: start;
        }
      }

      .progress {
        flex: 1;
        background-color: black;
        height: 10px;
        margin-inline: var(--sl-spacing-x-small);
        position: relative;

        .buffer {
          background-color: var(--sl-color-neutral-100);
          position: absolute;
          top: 0;
          bottom: 0;
        }

        .played {
          background-color: var(--sl-color-neutral-900);
          position: absolute;
          top: 0;
          left: 0;
          bottom: 0;
        }

        .mask {
          position: absolute;
          inset: -5px;
          background-color: transparent;
          border-color: var(--sl-color-neutral-300);
          border-style: solid;
          border-width: 5px;
          border-radius: 10px;
        }
      }
    `,
  ];

  static properties = {
    currentTime: { type: Number },
    isPlaying: { state: true },
    isFullscreen: { state: true },
    isCasting: { state: true },
    server: { type: String },
    video: { type: String },
    image: { type: String },
    url: { type: String },
    mimeType: { type: String },
    duration: { type: Number },
  };

  constructor() {
    super();
    this.currentTime = 0;
    this.lastReportedTime = 0;
    this.isPlaying = false;
    this.onFullscreenChanged();
    this.isCasting = false;
    this.videoElement = null;

    this.seekBack30 = (event) => this.seekPressed(event, -30);
    this.seekBack10 = (event) => this.seekPressed(event, -10);
    this.seekForward10 = (event) => this.seekPressed(event, 10);
    this.seekForward30 = (event) => this.seekPressed(event, 30);

    this.addEventListener("fullscreenchange", () => this.onFullscreenChanged());
  }

  renderedVideo(element) {
    if (this.videoElement != element) {
      this.videoElement?.pause();
    }

    this.videoElement = element;

    if (this.videoElement) {
      this.videoElement.currentTime = this.currentTime;
      if (this.isPlaying) {
        this.videoElement.play();
      } else {
        this.videoElement.pause();
      }
    }
  }

  connectedCallback() {
    super.connectedCallback();

    let url = new URL(document.documentURI);
    let castDevice = url.searchParams.get("cast");

    if (castDevice) {
      this.isCasting = true;
      this.initiateCastConnection(castDevice);
    }
  }

  disconnectedCallback() {
    this.videoElement?.pause();

    super.disconnectedCallback();
  }

  togglePlayback(event) {
    if (event.button != 0) {
      return;
    }

    event.stopPropagation();

    if (this.isCasting) {
      this.sendCastRequest({ type: this.isPlaying ? "pause" : "play" });
    } else if (this.isPlaying) {
      this.videoElement.pause();
    } else {
      this.videoElement.play();
    }
  }

  showOverlay() {
    let overlay = this.renderRoot.querySelector(".overlay");
    overlay.classList.add("visible");

    if (this._overlayTimeout) {
      clearTimeout(this._overlayTimeout);
    }

    this._overlayTimeout = setTimeout(() => {
      this._overlayTimeout = null;
      overlay.classList.remove("visible");
    }, 3000);
  }

  onFullscreenChanged() {
    this.isFullscreen = document.fullscreenElement == this;
  }

  async toggleFullscreen(event) {
    if (event.button != 0) {
      return;
    }

    try {
      this.isFullscreen = !this.isFullscreen;
      if (!this.isFullscreen) {
        await document.exitFullscreen();
      } else {
        await this.requestFullscreen();
      }
    } catch (e) {
      console.error(e);
      this.onFullscreenChanged();
    }
  }

  updateTime(newTime) {
    if (Math.abs(this.lastReportedTime - newTime) > 10) {
      this.lastReportedTime = newTime;

      let updateUrl = document.documentURI.replace("/library/", "/playback/");
      fetch(`${updateUrl}?position=${this.currentTime}`, {
        method: "POST",
      }).catch((e) => {
        console.error(e);
      });
    }

    this.currentTime = newTime;
  }

  onMediaStateChanged() {
    if (!this.videoElement) {
      return;
    }

    this.isPlaying = !(this.videoElement.paused || this.videoElement.ended);
    this.updateTime(this.videoElement.currentTime);
  }

  onMediaEnded() {
    this.onMediaStateChanged();
  }

  async seek(targetTime) {
    if (targetTime < 0 || targetTime >= this.duration) {
      return;
    }

    if (this.isCasting) {
      this.sendCastRequest({ type: "seek", position: targetTime });
    } else {
      this.currentTime = targetTime;
      this.videoElement.currentTime = targetTime;
    }
  }

  onProgressClicked(event) {
    if (event.button != 0) {
      return;
    }

    let progress = this.renderRoot.querySelector(".progress");
    let { x: elementX, width: elementWidth } = progress.getBoundingClientRect();

    let offset = event.clientX - elementX;

    let targetTime = (this.duration * offset) / elementWidth;
    this.seek(targetTime);
  }

  renderBuffers() {
    let ranges = this.videoElement?.buffered;
    if (!ranges) {
      return [];
    }

    let templates = [];
    for (let i = 0; i < ranges.length; i++) {
      let width = (100 * (ranges.end(i) - ranges.start(i))) / this.duration;
      let left = (100 * ranges.start(i)) / this.duration;

      templates.push(
        html`<div
          class="buffer"
          style="left: ${left}%; width: ${width}%"
        ></div>`,
      );
    }

    return templates;
  }

  seekPressed(event, delta) {
    if (event.button != 0) {
      return;
    }

    event.stopPropagation();
    this.seek(Math.max(0, this.currentTime + delta));
  }

  sendCastRequest(data) {
    if (!this.castConnection) {
      console.warn(
        "Attempting to send cast request after cast device has disconnected",
      );
      return;
    }

    console.log("Sending cast request", data);

    this.castConnection.send(JSON.stringify(data));
  }

  initialCastMessage = (event) => {
    if (event.target != this.castConnection) {
      return;
    }

    let message = JSON.parse(event.data);
    console.log("Initial cast event", message);

    this.castConnection.addEventListener("message", this.onCastMessage);

    if (message.type == "castState") {
      // If the cast device is already playing the current video then just update our state.
      if (message.server == this.server && message.video == this.video) {
        this.onCastMessage(event);
        return;
      }
    }

    this.sendCastRequest({
      type: "startVideo",
      server: this.server,
      video: this.video,
      position: this.currentTime,
    });
  };

  castTick = () => {
    this.currentTime =
      this.castLastPosition + (Date.now() - this.castBaseTime) / 1000;
  };

  onCastMessage = (event) => {
    if (event.target != this.castConnection) {
      return;
    }

    let message = JSON.parse(event.data);
    console.log("Cast event", message);

    if (
      message.type == "castState" &&
      message.server == this.server &&
      message.video == this.video
    ) {
      this.isCasting = true;
      this.isPlaying =
        message.state == "playing" || message.state == "buffering";
      this.castLastPosition = message.position;
      this.castBaseTime = Date.now();
      this.currentTime = message.position;

      if (message.state == "playing") {
        if (!this.castTimer) {
          this.castTimer = setInterval(this.castTick, 250);
        }
      } else if (this.castTimer) {
        clearInterval(this.castTimer);
        this.castTimer = null;
      }
    } else {
      this.onCastDisconnect();
    }
  };

  onCastDisconnect = () => {
    if (this.castConnection) {
      console.log("Disconnecting cast device");

      this.castConnection.removeEventListener(
        "message",
        this.initialCastMessage,
      );
      this.castConnection.removeEventListener("message", this.onCastMessage);
      this.castConnection.removeEventListener("close", this.onCastDisconnect);
      this.castConnection.removeEventListener("error", this.onCastError);
      this.castConnection = null;
    }

    if (this.castTimer) {
      clearInterval(this.castTimer);
      this.castTimer = null;
    }

    this.isCasting = false;

    let url = new URL(document.documentURI);
    url.searchParams.delete("cast");
    window.history.replaceState(null, null, url);
  };

  onCastError = (event) => {
    if (event.target != this.castConnection) {
      return;
    }

    console.error("Cast connection error");
    this.onCastDisconnect();
  };

  initiateCastConnection(id) {
    if (this.castConnection) {
      this.sendCastRequest({ type: "stop" });
      this.onCastDisconnect();
    }

    let socketUrl = new URL("/cast", document.documentURI);
    socketUrl.protocol = "ws";
    socketUrl.searchParams.set("device", id);

    console.log(`Initiating websocket request to ${socketUrl}`);
    this.castConnection = new WebSocket(socketUrl);

    this.castConnection.addEventListener("message", this.initialCastMessage, {
      once: true,
    });
    this.castConnection.addEventListener("close", this.onCastDisconnect);
    this.castConnection.addEventListener("error", this.onCastError);

    let url = new URL(document.documentURI);
    url.searchParams.set("cast", id);
    window.history.replaceState(null, null, url);
  }

  castDeviceSelected(event) {
    if (event.target.value) {
      this.initiateCastConnection(event.target.value);
    }
  }

  stopCasting() {
    this.sendCastRequest({ type: "stop" });
    this.onCastDisconnect();
  }

  renderOverlayInfo() {
    if (this.isPlaying && !this.isCasting) {
      return nothing;
    }

    let imageUrl = new URL(this.image);

    return html`
      <div class="info">
        <img src="${imageUrl.pathname}" />
      </div>
    `;
  }

  renderVideo() {
    if (this.isCasting) {
      return nothing;
    }

    let mediaUrl = new URL(this.url, document.documentURI);
    mediaUrl = new URL(mediaUrl.pathname, document.documentURI);

    return keyed(
      mediaUrl.toString(),
      html`
      <video
        ${ref(this.renderedVideo)}
        autoplay
        preload="auto"
        @ended="${this.onMediaEnded}"
        @pause="${this.onMediaStateChanged}"
        @play="${this.onMediaStateChanged}"
        @timeupdate="${this.onMediaStateChanged}"
        @seeked="${this.onMediaStateChanged}"
      >
        <source type="${this.mimeType}" src="${mediaUrl.toString()}"></source>
      </video>`,
    );
  }

  render() {
    let toggleIcon = this.isPlaying ? "pause_round" : "play_arrow_round";
    let fullscreenIcon = this.isFullscreen ? "fullscreen_exit" : "fullscreen";

    let playedPercent = (100 * this.currentTime) / this.duration;

    let classes = {
      overlay: true,
      casting: this.isCasting,
      playing: this.isPlaying,
      paused: !this.isPlaying,
    };

    return html`
      ${this.renderVideo()}
      <div class=${classMap(classes)} @mousemove="${this.showOverlay}">
        ${this.renderOverlayInfo()}
        <div class="main" @click="${this.togglePlayback}">
          <sl-icon-button
            library="material"
            name="replay_30"
            @click="${this.seekBack30}"
          ></sl-icon-button>
          <sl-icon-button
            library="material"
            name="replay_10"
            @click="${this.seekBack10}"
          ></sl-icon-button>
          <sl-icon-button
            class="togglePlayback"
            library="material"
            name="${toggleIcon}"
            @click="${this.togglePlayback}"
          ></sl-icon-button>
          <sl-icon-button
            library="material"
            name="forward_10"
            @click="${this.seekForward10}"
          ></sl-icon-button>
          <sl-icon-button
            library="material"
            name="forward_30"
            @click="${this.seekForward30}"
          ></sl-icon-button>
        </div>
        <div class="controls">
          <sl-icon-button
            library="material"
            name="${toggleIcon}"
            @click="${this.togglePlayback}"
          ></sl-icon-button>
          <div class="time start">${formatTime(this.currentTime)}</div>
          <div class="progress" @click="${this.onProgressClicked}">
            ${this.renderBuffers()}
            <div class="played" style="width: ${playedPercent}%"></div>
            <div class="mask"></div>
          </div>
          <div class="time end">
            -${formatTime(this.duration - this.currentTime)}
          </div>
          ${this.isCasting
            ? html`<sl-icon-button
                library="material"
                name="cast_connected"
                @click="${this.stopCasting}"
              ></sl-icon-button>`
            : html`<sl-dropdown
                placement="top-end"
                @click=${this.castDeviceSelected}
              >
                <sl-icon-button
                  slot="trigger"
                  library="material"
                  name="cast"
                ></sl-icon-button>
                <slot name="cast-devices"></slot>
              </sl-dropdown>`}
          ${!this.isCasting
            ? html`<sl-icon-button
                library="material"
                name="${fullscreenIcon}"
                @click="${this.toggleFullscreen}"
              ></sl-icon-button>`
            : nothing}
        </div>
      </div>
    `;
  }
}

customElements.define("video-player", VideoPlayer);
