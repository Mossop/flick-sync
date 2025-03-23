import { LitElement, html, css, styleMap, nothing } from "lit";

import reset from "reset";

export class VideoPlayer extends LitElement {
  static styles = [
    reset,
    css`
      :host {
      }

      a {
        width: 100%;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        gap: var(--sl-spacing-x-small);
        padding: var(--sl-spacing-x-small);
      }

      .thumbnail {
        width: 100%;
        aspect-ratio: 1;
        position: relative;
        overflow: hidden;
      }

      img {
        display: block;
        height: 100%;
        width: 100%;
        object-fit: contain;
        object-position: center center;
      }

      .overlay {
        position: absolute;
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

      .unplayed {
        margin: var(--sl-spacing-x-small);
        height: 16px;
        width: 16px;
        border-radius: 50%;
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
    overlayPosition: { state: true },
  };

  constructor() {
    super();

    this.overlayPosition = null;
  }

  onImageLoad(event) {
    let { naturalHeight, naturalWidth } = event.target;
    if (naturalHeight > naturalWidth) {
      let offset = (50 * (naturalHeight - naturalWidth)) / naturalHeight;

      this.overlayPosition = {
        inset: `0 ${offset}%`,
      };
    } else {
      let offset = (50 * (naturalWidth - naturalHeight)) / naturalWidth;

      this.overlayPosition = {
        inset: `${offset}% 0`,
      };
    }
  }

  render() {
    let overlayStyles = this.overlayPosition
      ? styleMap(this.overlayPosition)
      : "display: none";

    return html`
      <a class="grid-item" href="${this.url}">
        <div class="thumbnail">
          <img src="${this.image}" @load="${this.onImageLoad}" />
          <div class="overlay" style=${overlayStyles}>
            <div class="overlay-top">
              ${this.position == 0.0
                ? html`<div class="unplayed"></div>`
                : nothing}
            </div>
            <div class="overlay-bottom">
              ${this.position > 0.5 && this.position < 99.5
                ? html`<div
                    class="progress"
                    style="width: ${this.position}%"
                  ></div>`
                : nothing}
            </div>
          </div>
        </div>
        <p>${this.name}</p>
      </a>
    `;
  }
}

customElements.define("grid-thumbnail", VideoPlayer);
