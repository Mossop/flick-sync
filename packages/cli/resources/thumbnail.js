import { LitElement, html, css, styleMap, nothing, classMap } from "lit";

import reset from "reset";

export class VideoPlayer extends LitElement {
  static styles = [
    reset,
    css`
      :host {
      }

      a {
        height: 100%;
        width: 100%;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--sl-spacing-x-small);
        padding: var(--sl-spacing-x-small);
      }

      .thumbnail {
        flex: 1;
        width: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
      }

      .image {
        position: relative;
      }

      img {
        display: block;
        max-height: 150px;
        max-width: 150px;
      }

      .overlay {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        justify-content: space-between;
      }

      .overlay-top {
        display: flex;
        flex-direction: row;
        justify-content: end;
      }

      .playstate {
        margin: var(--sl-spacing-x-small);
        height: 16px;
        width: 16px;
        border-radius: 50%;
        background-color: var(--sl-color-neutral-600);

        &:hover {
          background-color: var(--sl-color-primary-700);
        }
      }

      .unplayed {
        background-color: var(--sl-color-primary-600);
      }

      .overlay-bottom {
      }

      .progress {
        height: 5px;
        align-self: start;
        background-color: var(--sl-color-primary-600);
      }

      p {
        text-overflow: ellipsis;
        text-align: center;
        overflow: hidden;
        white-space: nowrap;
      }
    `,
  ];

  static properties = {
    name: { type: String },
    image: { type: String },
    url: { type: String },
    position: { type: Object },
    duration: { type: Object },
  };

  constructor() {
    super();
  }

  get percentPlayed() {
    if (this.position === null || this.duration === null) {
      return null;
    }

    return (100 * this.position) / this.duration;
  }

  togglePlayState(event) {
    event.preventDefault();
    event.stopPropagation();

    let percent = this.percentPlayed;
    let newTime;
    if (percent == 100) {
      newTime = 0;
    } else if (percent == 0 || percent > 20) {
      newTime = this.duration;
    } else {
      newTime = 0;
    }

    let updateUrl = new URL(
      this.url.replace("/library/", "/playback/"),
      document.documentURI
    );

    fetch(`${updateUrl}?position=${newTime}`, {
      method: "POST",
    }).catch((e) => {
      console.error(e);
    });
  }

  renderPlayedDot() {
    if (this.position === null) {
      return nothing;
    }

    let classes = {
      playstate: true,
      unplayed: this.percentPlayed <= 0.5,
    };

    return html`<div
      @click="${this.togglePlayState}"
      class=${classMap(classes)}
    ></div>`;
  }

  render() {
    return html`
      <a class="grid-item" href="${this.url}">
        <div class="thumbnail">
          <div class="image">
            <img src="${this.image}" />
            <div class="overlay">
              <div class="overlay-top">${this.renderPlayedDot()}</div>
              <div class="overlay-bottom">
                ${this.percentPlayed > 0.5 && this.percentPlayed < 99.5
                  ? html`<div
                      class="progress"
                      style="width: ${this.percentPlayed}%"
                    ></div>`
                  : nothing}
              </div>
            </div>
          </div>
        </div>
        <p>${this.name}</p>
      </a>
    `;
  }
}

customElements.define("grid-thumbnail", VideoPlayer);
