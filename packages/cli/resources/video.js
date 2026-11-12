import { LitElement, html, css, nothing, ref, keyed, classMap } from "lit";
import shoelaceStyles from "@shoelace/themes/dark.styles.js";
import "@shoelace/components/icon-button/icon-button.js";
import "@shoelace/components/dropdown/dropdown.js";
import "@shoelace/components/button/button.js";

// User adjustable delay applied to the local audio, in milliseconds.
// Local audio settings are stored per cast device under this prefix followed by
// the device ID.
const AUDIO_SETTINGS_KEY_PREFIX = "castAudio:";
const AUDIO_OFFSET_STEP = 50;
// Drift (in seconds) beyond which the local audio is seeked rather than
// gradually corrected.
const AUDIO_SEEK_THRESHOLD = 0.25;
// Drift (in seconds) beyond which the local audio's playback rate is adjusted
// to bring it back in sync. Audio ahead of the picture is noticed sooner than
// audio behind it (ITU-R BT.1359 gives roughly 45ms ahead and 125ms behind).
const AUDIO_CORRECTION_START_AHEAD = 0.05;
const AUDIO_CORRECTION_START_BEHIND = 0.1;
// Once correcting, drift (in seconds) below which the playback rate returns to
// normal.
const AUDIO_CORRECTION_STOP = 0.015;
// The playback rate adjustment used while correcting drift. Pitch preservation
// is disabled as it causes audible warbling so this also shifts the pitch,
// 1% is roughly 17 cents.
const AUDIO_RATE_ADJUST = 0.01;
// Minimum time (in milliseconds) after the playback rate last changed before it
// can be changed again. Seeks ignore this.
const AUDIO_RATE_CHANGE_DELAY = 5000;
// Minimum time (in milliseconds) after the last seek before drift can be
// corrected by seeking again.
const AUDIO_SEEK_DELAY = 5000;
// Time (in milliseconds) after a seek completes at which the remaining drift is
// measured to learn how far ahead seeks need to aim.
const AUDIO_SEEK_CHECK_DELAY = 2000;
// Time (in milliseconds) after which a seek that hasn't completed is no longer
// waited for.
const AUDIO_SEEK_TIMEOUT = 5000;
// Time (in milliseconds) to wait for the cast device to pause after asking it
// to before the local audio follows the device again.
const AUDIO_PAUSE_TIMEOUT = 5000;
// Maximum amount (in seconds) that seeks will aim ahead of the target.
const AUDIO_MAX_SEEK_LEAD = 0.5;
// Initial amount (in seconds) that seeks aim ahead of the target until a better
// value is learned. Seeks have been seen to lose around 250ms.
const AUDIO_DEFAULT_SEEK_LEAD = 0.25;
// Positions reported by the cast device jitter and are occasionally far out so
// while playing the estimated position is based on the median of this many
// recent reports.
const CAST_POSITION_SAMPLES = 9;
// The number of reports needed before the median is used.
const CAST_POSITION_MIN_SAMPLES = 3;
// The position reported when playback starts can be inaccurate. The server
// requests a fresh status a second later so for this long (in milliseconds)
// after playback starts reported positions are used as is.
const CAST_POSITION_SETTLE_TIME = 3000;

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

function median(values) {
  let sorted = [...values].sort((a, b) => a - b);
  let middle = Math.floor(sorted.length / 2);

  if (sorted.length % 2) {
    return sorted[middle];
  }

  return (sorted[middle - 1] + sorted[middle]) / 2;
}

function formatMs(seconds) {
  return `${(seconds * 1000).toFixed(0)}ms`;
}

function log(...args) {
  let now = new Date();
  let time = [now.getHours(), now.getMinutes(), now.getSeconds()]
    .map(pad)
    .join(":");
  let millis = now.getMilliseconds().toString().padStart(3, "0");
  console.log(`[${time}.${millis}]`, ...args);
}

function showError(message) {
  let alert = document.createElement("sl-alert");
  alert.variant = "danger";
  alert.closable = true;
  alert.duration = 15000;
  alert.textContent = message;
  document.body.append(alert);
  customElements.whenDefined("sl-alert").then(() => alert.toast());
}

class PlaybackDevice {
  #player = null;

  constructor(player) {
    this.#player = player;
  }

  get isCasting() {
    return false;
  }

  buffers() {
    return null;
  }

  get player() {
    return this.#player;
  }

  get currentTime() {
    return this.#player.currentTime;
  }

  set currentTime(val) {
    this.#player.currentTime = val;
  }

  rendered() {}

  render() {
    return nothing;
  }

  renderControls() {
    return nothing;
  }

  // Called when the player is removed from the page.
  detach() {}
}

class CastPlayback extends PlaybackDevice {
  #castState;
  #castConnection = null;
  #castLastPosition;
  #castBaseTime;
  #castTimer;
  // True after asking the device to start this video until it reports that it
  // is playing it.
  #startingVideo = false;
  // When true the next reported position is used as is rather than smoothed.
  #snapPosition = true;
  // Until this time reported positions are used as is.
  #snapUntil = 0;
  // Recent reported positions while playing, as offsets in seconds from the
  // wall clock.
  #positionOffsets = [];
  #audioElement;
  #audioOffset = 0;
  #audioSeekStart = null;
  #audioLastSeekAt = -Infinity;
  #audioSeekLead = AUDIO_DEFAULT_SEEK_LEAD;
  #audioSeekLearn = false;
  #audioSeekCheckAt = null;
  #audioRateChangedAt = -Infinity;
  #audioPauseRequestedAt = -Infinity;
  #deviceId;

  constructor(player, device) {
    super(player);
    this.#deviceId = device;
    this.#castState = "idle";
    this.#castBaseTime = Date.now();
    this.#castLastPosition = this.currentTime;
    this.localAudio = false;
    this.#loadAudioSettings();
    this.#initiateCastConnection(device);
    this.#castTimer = setInterval(this.#castTick, 250);
  }

  #loadAudioSettings() {
    try {
      let settings = JSON.parse(
        localStorage.getItem(AUDIO_SETTINGS_KEY_PREFIX + this.#deviceId),
      );
      if (settings) {
        this.localAudio = !!settings.localAudio;
        this.#audioOffset = parseInt(settings.offset, 10) || 0;
      }
    } catch (e) {
      console.warn("Unable to read local audio settings", e);
    }
  }

  #saveAudioSettings() {
    try {
      localStorage.setItem(
        AUDIO_SETTINGS_KEY_PREFIX + this.#deviceId,
        JSON.stringify({
          localAudio: this.localAudio,
          offset: this.#audioOffset,
        }),
      );
    } catch (e) {
      console.warn("Unable to store local audio settings", e);
    }
  }

  get isCasting() {
    return true;
  }

  get currentTime() {
    return super.currentTime;
  }

  set currentTime(val) {
    this.#castBaseTime = Date.now();
    this.#castLastPosition = val;
    super.currentTime = val;
  }

  #estimateTime() {
    if (this.#castState == "playing") {
      return this.#castLastPosition + (Date.now() - this.#castBaseTime) / 1000;
    }

    return this.#castLastPosition;
  }

  #castTick = () => {
    super.currentTime = this.#estimateTime();
    this.#syncAudio();
  };

  // Updates the estimated position from a position reported by the cast device.
  // Must be called before the new cast state is applied.
  #updatePosition(position, state) {
    let now = Date.now();
    let offset = position - now / 1000;

    if (now < this.#snapUntil && state == "playing") {
      log(
        `Audio sync: cast position ${formatMs(position - this.#estimateTime())} from estimate, using as is after playback started`,
      );
      this.#positionOffsets = [offset];
      this.currentTime = position;
      return;
    }

    if (
      this.#snapPosition ||
      this.#castState != "playing" ||
      state != "playing"
    ) {
      if (state == "playing" && this.#castState != "playing") {
        this.#snapUntil = now + CAST_POSITION_SETTLE_TIME;
      }

      this.#snapPosition = false;
      this.#positionOffsets = state == "playing" ? [offset] : [];
      this.currentTime = position;
      return;
    }

    this.#positionOffsets.push(offset);
    if (this.#positionOffsets.length > CAST_POSITION_SAMPLES) {
      this.#positionOffsets.shift();
    }

    let estimate = this.#estimateTime();
    let error = position - estimate;

    if (this.#positionOffsets.length < CAST_POSITION_MIN_SAMPLES) {
      log(
        `Audio sync: cast position ${formatMs(error)} from estimate, collecting reports (${this.#positionOffsets.length}/${CAST_POSITION_MIN_SAMPLES})`,
      );
      return;
    }

    let newEstimate = median(this.#positionOffsets) + Date.now() / 1000;
    log(
      `Audio sync: cast position ${formatMs(error)} from estimate, adjusting by ${formatMs(newEstimate - estimate)}`,
    );
    this.currentTime = newEstimate;
  }

  // Where the local audio should be right now.
  #audioTarget() {
    return Math.max(0, this.#estimateTime() - this.#audioOffset / 1000);
  }

  // True between asking the cast device to pause and it reporting that it has.
  #audioPausePending() {
    return (
      performance.now() - this.#audioPauseRequestedAt < AUDIO_PAUSE_TIMEOUT
    );
  }

  #updateAudioPlayState() {
    if (this.#castState != "playing") {
      this.#audioPauseRequestedAt = -Infinity;
    }

    if (!this.#audioElement) {
      return;
    }

    if ("mediaSession" in navigator) {
      navigator.mediaSession.playbackState = this.player.isPlaying
        ? "playing"
        : "paused";
    }

    if (this.#castState == "playing" && !this.#audioPausePending()) {
      if (this.#audioElement.paused) {
        log(`Audio sync: playing local audio`);

        // Starting playback loses time in the same way as seeking so aim
        // ahead by the learned amount. Only learn from this if the audio was
        // ready to play, otherwise the time taken to load would be included.
        let learn =
          this.#audioElement.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA;
        this.#seekAudio(
          this.#audioTarget() + this.#audioSeekLead,
          `starting playback, leading by ${formatMs(this.#audioSeekLead)}`,
          learn,
        );
      }
      this.#audioElement.play().catch((e) => {
        console.warn("Unable to play local audio", e);
      });
    } else {
      if (!this.#audioElement.paused) {
        log(
          `Audio sync: pausing local audio, cast state is ${this.#castState}`,
        );
      }
      this.#audioElement.pause();
    }

    this.#syncAudio();
  }

  // When learn is true the drift shortly after the seek is used to adjust how
  // far ahead future seeks aim.
  #seekAudio(position, reason, learn = false) {
    log(
      `Audio sync: seeking from ${this.#audioElement.currentTime.toFixed(3)}s to ${position.toFixed(3)}s (${reason})`,
    );
    this.#audioSeekStart = performance.now();
    this.#audioLastSeekAt = this.#audioSeekStart;
    this.#audioSeekLearn = learn;
    this.#audioSeekCheckAt = null;
    this.#setAudioRate(1, 0, true);
    this.#audioElement.currentTime = position;
  }

  #setAudioRate(rate, drift, force = false) {
    if (this.#audioElement.playbackRate == rate) {
      return;
    }

    // Frequent rate changes are audible so don't change the rate again too
    // soon after the last change.
    if (
      !force &&
      performance.now() - this.#audioRateChangedAt < AUDIO_RATE_CHANGE_DELAY
    ) {
      return;
    }

    this.#audioRateChangedAt = performance.now();

    if (rate == 1) {
      log(`Audio sync: playback rate reset to 1`);
    } else {
      log(
        `Audio sync: playback rate ${rate.toFixed(4)}, drift ${(drift * 1000).toFixed(0)}ms`,
      );
    }

    this.#audioElement.playbackRate = rate;
  }

  #onAudioSeeked = () => {
    if (this.#audioSeekStart !== null) {
      let duration = (performance.now() - this.#audioSeekStart) / 1000;
      this.#audioSeekStart = null;
      log(
        `Audio sync: seek took ${formatMs(duration)}, drift now ${formatMs(this.#audioTarget() - this.#audioElement.currentTime)}`,
      );

      if (this.#audioSeekLearn) {
        this.#audioSeekCheckAt = performance.now() + AUDIO_SEEK_CHECK_DELAY;
      }
    }
  };

  // Called a short time after a seek to learn how far ahead seeks need to aim
  // to end up in sync. Returns true while waiting for that time.
  #checkSeekLead(drift) {
    if (this.#audioSeekCheckAt === null) {
      return false;
    }

    if (this.#castState != "playing") {
      this.#audioSeekCheckAt = null;
      return false;
    }

    if (performance.now() < this.#audioSeekCheckAt) {
      return true;
    }

    this.#audioSeekCheckAt = null;
    this.#audioSeekLead = Math.max(
      0,
      Math.min(AUDIO_MAX_SEEK_LEAD, this.#audioSeekLead + drift),
    );
    log(
      `Audio sync: drift ${formatMs(drift)} after seek, seek lead now ${formatMs(this.#audioSeekLead)}`,
    );
    return false;
  }

  // Keeps the local audio in step with the cast device. Small amounts of drift
  // are corrected by adjusting the playback rate to avoid audible jumps, larger
  // amounts require a seek.
  #syncAudio() {
    let audio = this.#audioElement;
    if (!audio || audio.seeking || this.#audioPausePending()) {
      return;
    }

    // Before the audio has loaded setting its position doesn't start a seek
    // straight away so also wait for our last seek to complete.
    if (
      this.#audioSeekStart !== null &&
      performance.now() - this.#audioSeekStart < AUDIO_SEEK_TIMEOUT
    ) {
      return;
    }

    let target = this.#audioTarget();
    let drift = target - audio.currentTime;

    if (this.#castState != "playing") {
      this.#setAudioRate(1, drift);
      if (Math.abs(drift) > AUDIO_CORRECTION_START_AHEAD) {
        this.#seekAudio(
          target,
          `${this.#castState}, drift ${(drift * 1000).toFixed(0)}ms`,
        );
      }
      return;
    }

    // Don't correct anything until the result of the last seek is known.
    if (this.#checkSeekLead(drift)) {
      return;
    }

    if (
      Math.abs(drift) > AUDIO_SEEK_THRESHOLD &&
      performance.now() - this.#audioLastSeekAt >= AUDIO_SEEK_DELAY
    ) {
      // The audio loses time while seeking so aim ahead by the learned amount.
      this.#seekAudio(
        target + this.#audioSeekLead,
        `drift ${formatMs(drift)}, leading by ${formatMs(this.#audioSeekLead)}`,
        true,
      );
    } else if (
      // A positive drift means the audio is behind.
      Math.abs(drift) >
        (drift > 0
          ? AUDIO_CORRECTION_START_BEHIND
          : AUDIO_CORRECTION_START_AHEAD) ||
      (audio.playbackRate != 1 && Math.abs(drift) > AUDIO_CORRECTION_STOP)
    ) {
      // Changing the playback rate can cause audible glitches so only use a
      // fixed rate in either direction until back in sync.
      let rate = drift > 0 ? 1 + AUDIO_RATE_ADJUST : 1 - AUDIO_RATE_ADJUST;
      this.#setAudioRate(rate, drift);
    } else {
      this.#setAudioRate(1, drift);
    }
  }

  adjustAudioOffset(delta) {
    this.#audioOffset += delta;
    log(`Audio sync: offset changed to ${this.#audioOffset}ms`);
    this.#saveAudioSettings();
    this.player.requestUpdate();
  }

  #sendCastRequest(data) {
    if (!this.#castConnection) {
      console.warn(
        "Attempting to send cast request after cast device has disconnected",
      );
      return;
    }

    log("Sending cast request", data);

    this.#castConnection.send(JSON.stringify(data));
  }

  #initialCastMessage = (event) => {
    if (event.target != this.#castConnection) {
      return;
    }

    let message = JSON.parse(event.data);
    log("Initial cast event", message);

    this.#castConnection.addEventListener("message", this.#onCastMessage);

    // Local audio was left on the last time this device was used.
    if (this.localAudio) {
      log("Audio sync: local audio restored on");
      this.#sendCastRequest({ type: "setMuted", muted: true });
    }

    if (message.type == "castState") {
      // If the cast device is already playing the current video then just update our state.
      if (
        message.server == this.player.server &&
        message.video == this.player.video
      ) {
        this.#onCastMessage(event);
        return;
      }
    }

    this.#startingVideo = true;
    this.#sendCastRequest({
      type: "startVideo",
      server: this.player.server,
      video: this.player.video,
      position: this.currentTime,
    });
  };

  #onCastMessage = (event) => {
    if (event.target != this.#castConnection) {
      return;
    }

    let message = JSON.parse(event.data);
    log("Cast event", message);

    if (message.type == "error") {
      console.error("Cast error", message.message);
      showError(message.message);
      this.#onCastDisconnect();
      return;
    }

    if (
      message.type == "castState" &&
      message.server == this.player.server &&
      message.video == this.player.video
    ) {
      this.#startingVideo = false;
      this.#updatePosition(message.position, message.state);
      this.#castState = message.state;
      this.player.isPlaying =
        message.state == "playing" || message.state == "buffering";
      this.#updateAudioPlayState();
    } else if (message.type == "castState" && this.#startingVideo) {
      // The device may still report the media it was previously playing
      // until our video has loaded.
      log("Ignoring cast state for other media while starting video");
    } else {
      this.#onCastDisconnect();
    }
  };

  // Stops listening to the cast connection and stops the local audio.
  #shutdown() {
    if (this.#castConnection) {
      this.#castConnection.removeEventListener(
        "message",
        this.#initialCastMessage,
      );
      this.#castConnection.removeEventListener("message", this.#onCastMessage);
      this.#castConnection.removeEventListener("close", this.#onCastDisconnect);
      this.#castConnection.removeEventListener("error", this.#onCastError);
      this.#castConnection.close();
      this.#castConnection = null;
    }

    if (this.#castTimer) {
      clearInterval(this.#castTimer);
      this.#castTimer = null;
    }

    if (this.#audioElement) {
      this.#audioElement.pause();
      this.#removeMediaSessionHandlers();
    }
  }

  // While local audio is playing, media controls such as those on bluetooth
  // headphones act on it. Rather than letting the browser pause or play the
  // audio directly pass the request on to the cast device, the local audio
  // follows once the device reports its new state.
  #installMediaSessionHandlers() {
    if (!("mediaSession" in navigator)) {
      return;
    }

    navigator.mediaSession.setActionHandler("play", () => {
      log("Audio sync: media session play");
      this.play();
    });
    navigator.mediaSession.setActionHandler("pause", () => {
      log("Audio sync: media session pause");
      this.pause();
    });
  }

  #removeMediaSessionHandlers() {
    if (!("mediaSession" in navigator)) {
      return;
    }

    navigator.mediaSession.setActionHandler("play", null);
    navigator.mediaSession.setActionHandler("pause", null);
    navigator.mediaSession.playbackState = "none";
  }

  // Called when the player is removed from the page. The cast device carries on
  // playing.
  detach() {
    log("Detaching from cast device");
    this.#shutdown();
  }

  #onCastDisconnect = () => {
    if (this.#castConnection) {
      log("Disconnecting cast device");
    }

    this.#shutdown();

    let url = new URL(document.documentURI);
    url.searchParams.delete("cast");
    window.history.replaceState(null, null, url);

    this.player.playbackDevice = new LocalPlayback(this.player);
  };

  #onCastError = (event) => {
    if (event.target != this.#castConnection) {
      return;
    }

    console.error("Cast connection error");
    this.#onCastDisconnect();
  };

  #initiateCastConnection(id) {
    if (this.#castConnection) {
      this.#sendCastRequest({ type: "stop" });
      this.#onCastDisconnect();
    }

    let socketUrl = new URL("/cast", document.documentURI);
    socketUrl.protocol = "ws";
    socketUrl.searchParams.set("device", id);

    log(`Initiating websocket request to ${socketUrl}`);
    this.#castConnection = new WebSocket(socketUrl);

    this.#castConnection.addEventListener("message", this.#initialCastMessage, {
      once: true,
    });
    this.#castConnection.addEventListener("close", this.#onCastDisconnect);
    this.#castConnection.addEventListener("error", this.#onCastError);

    let url = new URL(document.documentURI);
    url.searchParams.set("cast", id);
    window.history.replaceState(null, null, url);
  }

  play() {
    this.#audioPauseRequestedAt = -Infinity;
    this.#sendCastRequest({ type: "play" });
  }

  pause() {
    this.#sendCastRequest({ type: "pause" });

    // The cast device takes a moment to pause so stop the local audio straight
    // away rather than waiting for it to report that it has paused.
    if (this.#audioElement && !this.#audioElement.paused) {
      log("Audio sync: pausing local audio, pause requested");
      this.#audioPauseRequestedAt = performance.now();
      this.#audioElement.pause();
    }
  }

  stop() {
    this.#sendCastRequest({ type: "stop" });
    this.#onCastDisconnect();
  }

  seek(newTime) {
    this.#snapPosition = true;
    this.#sendCastRequest({ type: "seek", position: newTime });
  }

  rendered() {
    let element = this.player.renderRoot.querySelector("audio");

    if (this.#audioElement === element) {
      return;
    }

    this.#audioElement?.pause();
    this.#audioElement = element;
    this.#audioSeekStart = null;
    this.#audioSeekCheckAt = null;

    if (!this.#audioElement) {
      this.#removeMediaSessionHandlers();
      return;
    }

    this.#installMediaSessionHandlers();

    this.#seekAudio(this.#audioTarget(), "audio element created");
    this.#updateAudioPlayState();
  }

  toggleLocalAudio() {
    this.localAudio = !this.localAudio;
    log(
      `Audio sync: local audio turned ${this.localAudio ? "on" : "off"}`,
    );
    this.#sendCastRequest({ type: "setMuted", muted: this.localAudio });
    this.#saveAudioSettings();
    this.player.requestUpdate();
  }

  renderControls() {
    let toggle = html`<sl-icon-button
      class=${classMap({ "local-audio": true, enabled: this.localAudio })}
      library="material"
      name="speaker_phone"
      label="Play the audio on this device"
      @click="${() => this.toggleLocalAudio()}"
    ></sl-icon-button>`;

    if (!this.localAudio) {
      return toggle;
    }

    let offset = this.#audioOffset;
    let label = `${offset > 0 ? "+" : ""}${offset}ms`;

    return html`${toggle}
      <div class="audio-offset" title="Local audio delay">
        <sl-button
          size="small"
          @click="${() => this.adjustAudioOffset(-AUDIO_OFFSET_STEP)}"
          >−</sl-button
        >
        <span>${label}</span>
        <sl-button
          size="small"
          @click="${() => this.adjustAudioOffset(AUDIO_OFFSET_STEP)}"
          >+</sl-button
        >
      </div>`;
  }

  render() {
    if (!this.localAudio) {
      return nothing;
    }

    let mediaUrl = new URL(this.player.url, document.documentURI);
    mediaUrl = new URL(mediaUrl.pathname, document.documentURI);

    return keyed(
      mediaUrl.toString(),
      html`
      <audio
        preload="auto"
        .preservesPitch=${false}
        @seeked="${this.#onAudioSeeked}"
      >
        <source type="${this.player.mimeType}" src="${mediaUrl.toString()}"></source>
      </audio>`,
    );
  }
}

class LocalPlayback extends PlaybackDevice {
  #videoElement = null;
  #lastReportedTime;

  constructor(player) {
    super(player);
    this.#lastReportedTime = this.currentTime;
  }

  rendered() {
    let element = this.player.renderRoot.querySelector("video");

    if (this.#videoElement === element) {
      return;
    }

    this.#videoElement?.pause();
    this.#videoElement = element;

    if (this.#videoElement) {
      this.#videoElement.currentTime = this.currentTime;
      if (this.player.isPlaying !== false) {
        this.#videoElement.play();
      } else {
        this.#videoElement.pause();
      }
    }
  }

  updateTime(newTime) {
    if (Math.abs(this.#lastReportedTime - newTime) > 10) {
      this.#lastReportedTime = newTime;

      let updateUrl = document.documentURI.replace("/library/", "/playback/");
      fetch(`${updateUrl}?position=${this.currentTime}`, {
        method: "POST",
      }).catch((e) => {
        console.error(e);
      });
    }

    this.currentTime = newTime;
  }

  onMediaStateChanged = () => {
    if (!this.#videoElement) {
      return;
    }

    this.player.isPlaying = !(
      this.#videoElement.paused || this.#videoElement.ended
    );
    this.updateTime(this.#videoElement.currentTime);
  };

  buffers() {
    return this.#videoElement?.buffered;
  }

  play() {
    this.#videoElement?.play();
  }

  pause() {
    this.#videoElement?.pause();
  }

  stop() {
    this.#videoElement?.pause();
  }

  detach() {
    this.#videoElement?.pause();
  }

  seek(newTime) {
    this.#videoElement.currentTime = newTime;
  }

  render() {
    let mediaUrl = new URL(this.player.url, document.documentURI);
    mediaUrl = new URL(mediaUrl.pathname, document.documentURI);

    return keyed(
      mediaUrl.toString(),
      html`
      <video
        autoplay
        preload="auto"
        @ended="${this.onMediaStateChanged}"
        @pause="${this.onMediaStateChanged}"
        @play="${this.onMediaStateChanged}"
        @timeupdate="${this.onMediaStateChanged}"
        @seeked="${this.onMediaStateChanged}"
      >
        <source type="${this.player.mimeType}" src="${mediaUrl.toString()}"></source>
      </video>`,
    );
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

      .local-audio::part(base) {
        color: var(--disconnected-color);
      }

      .local-audio.enabled::part(base) {
        color: var(--connected-color);
      }

      .audio-offset {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: var(--sl-spacing-2x-small);

        span {
          min-width: 4.5em;
          text-align: center;
          font-variant-numeric: tabular-nums;
        }
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
    playbackDevice: { state: true },
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
    this.isPlaying = null;
    this.onFullscreenChanged();

    this.seekBack30 = (event) => this.seekPressed(event, -30);
    this.seekBack10 = (event) => this.seekPressed(event, -10);
    this.seekForward10 = (event) => this.seekPressed(event, 10);
    this.seekForward30 = (event) => this.seekPressed(event, 30);
    this.keyPressed = (event) => this.#keyPressed(event);
    this.visibilityChanged = () =>
      log(`Visibility changed to ${document.visibilityState}`);

    this.addEventListener("fullscreenchange", () => this.onFullscreenChanged());
  }

  connectedCallback() {
    super.connectedCallback();

    let url = new URL(document.documentURI);
    let castDevice = url.searchParams.get("cast");

    if (castDevice) {
      this.playbackDevice = new CastPlayback(this, castDevice);
    } else {
      this.playbackDevice = new LocalPlayback(this);
    }

    document.addEventListener("keydown", this.keyPressed);
    document.addEventListener("visibilitychange", this.visibilityChanged);
  }

  disconnectedCallback() {
    super.disconnectedCallback();

    document.removeEventListener("keydown", this.keyPressed);
    document.removeEventListener("visibilitychange", this.visibilityChanged);

    this.playbackDevice?.detach();
  }

  #keyPressed(event) {
    let delta = event.shiftKey ? 30 : 10;

    switch (event.key) {
      case "ArrowLeft":
        this.seek(Math.max(0, this.currentTime - delta));
        break;
      case "ArrowRight":
        this.seek(Math.max(0, this.currentTime + delta));
        break;
      case " ":
        this.togglePlayback();
        break;
      default:
        return;
    }

    event.stopPropagation();
    event.preventDefault();
  }

  togglePlaybackClicked(event) {
    if (event.button != 0) {
      return;
    }

    event.stopPropagation();
    this.togglePlayback();
  }

  togglePlayback() {
    if (this.isPlaying !== true) {
      this.playbackDevice.play();
    } else {
      this.playbackDevice.pause();
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

  async seek(targetTime) {
    if (targetTime < 0 || targetTime >= this.duration) {
      return;
    }

    this.playbackDevice.seek(targetTime);
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
    let ranges = this.playbackDevice.buffers();
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

  castDeviceSelected(event) {
    if (event.target.value) {
      this.playbackDevice.stop();
      this.playbackDevice = new CastPlayback(this, event.target.value);
    }
  }

  stopCasting() {
    this.playbackDevice.stop();
  }

  renderOverlayInfo() {
    if (this.isPlaying && !this.playbackDevice.isCasting) {
      return nothing;
    }

    let imageUrl = new URL(this.image);

    return html`
      <div class="info">
        <img src="${imageUrl.pathname}" />
      </div>
    `;
  }

  updated() {
    this.playbackDevice.rendered();
  }

  render() {
    let toggleIcon = this.isPlaying ? "pause_round" : "play_arrow_round";
    let fullscreenIcon = this.isFullscreen ? "fullscreen_exit" : "fullscreen";

    let playedPercent = (100 * this.currentTime) / this.duration;

    let classes = {
      overlay: true,
      casting: this.playbackDevice.isCasting,
      playing: this.isPlaying,
      paused: !this.isPlaying,
    };

    return html`
      ${this.playbackDevice.render()}
      <div class=${classMap(classes)} @mousemove="${this.showOverlay}">
        ${this.renderOverlayInfo()}
        <div class="main" @click="${this.togglePlaybackClicked}">
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
            @click="${this.togglePlaybackClicked}"
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
            @click="${this.togglePlaybackClicked}"
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
          ${this.playbackDevice.renderControls()}
          ${this.playbackDevice.isCasting
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
          ${!this.playbackDevice.isCasting
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
