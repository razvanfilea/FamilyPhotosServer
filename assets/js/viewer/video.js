// A video slide with controls like Android's VideoPlayer: back 5 s / play-pause / forward 5 s in
// the centre, the time, a mute button and a seek slider at the bottom. Not autoplayed. The
// controls carry `viewer-chrome`, so they show and hide with the viewer's bars.

import { formatTime } from './math.js';

export const SEEK_STEP = 5;
const SEEK_MAX = 1000;

/**
 * @param {HTMLTemplateElement} template `[data-template="video"]` of the viewer root
 * @param {{id: number}} photo
 * @param {Record<string, string>} strings
 */
export function createVideo(template, photo, strings) {
    const element = /** @type {HTMLElement} */ (template.content.firstElementChild.cloneNode(true));
    const part = (name) => element.querySelector(`[data-video="${name}"]`);
    /** @type {HTMLVideoElement} */
    const video = element.querySelector('video');
    const playButton = part('play');
    const muteButton = part('mute');
    const time = part('time');
    const seek = part('seek');
    const error = part('error');
    let seeking = false;

    video.poster = `/api/preview/${photo.id}`;
    part('download').href = `/api/download/${photo.id}`;

    const duration = () => (Number.isFinite(video.duration) ? video.duration : 0);

    function showTime(current = video.currentTime) {
        time.textContent = `${formatTime(current)} / ${formatTime(duration())}`;
    }

    function syncPlaying() {
        const playing = !video.paused && !video.ended;
        element.classList.toggle('is-playing', playing);
        playButton.setAttribute('aria-label', playing ? strings.pause : strings.play);
    }

    function syncMuted() {
        muteButton.setAttribute('aria-pressed', String(video.muted));
        muteButton.setAttribute('aria-label', video.muted ? strings.unmute : strings.mute);
    }

    const toggle = () => {
        if (video.paused || video.ended) {
            video.play().catch(() => {});
        } else {
            video.pause();
        }
    };
    const seekBy = (seconds) => {
        video.currentTime = Math.min(duration(), Math.max(0, video.currentTime + seconds));
    };
    const toggleMute = () => {
        video.muted = !video.muted;
    };

    playButton.addEventListener('click', toggle);
    part('back').addEventListener('click', () => seekBy(-SEEK_STEP));
    part('forward').addEventListener('click', () => seekBy(SEEK_STEP));
    muteButton.addEventListener('click', toggleMute);

    seek.addEventListener('input', () => {
        seeking = true;
        showTime((Number(seek.value) / SEEK_MAX) * duration());
    });
    seek.addEventListener('change', () => {
        video.currentTime = (Number(seek.value) / SEEK_MAX) * duration();
        seeking = false;
    });

    video.addEventListener('timeupdate', () => {
        if (seeking) return;
        showTime();
        seek.value = String(duration() > 0 ? Math.round((video.currentTime / duration()) * SEEK_MAX) : 0);
    });
    video.addEventListener('loadedmetadata', () => showTime());
    video.addEventListener('durationchange', () => showTime());
    video.addEventListener('play', syncPlaying);
    video.addEventListener('pause', syncPlaying);
    video.addEventListener('ended', syncPlaying);
    video.addEventListener('volumechange', syncMuted);
    video.addEventListener('error', () => {
        // Errors of a released video (no source) aren't the codec's fault
        if (!video.getAttribute('src')) return;
        error.hidden = false;
        element.classList.add('has-error');
    });

    syncPlaying();
    syncMuted();

    return {
        element,
        video,
        /** Starts loading the metadata (and the poster); called for the current photo and its neighbours */
        load() {
            if (!video.getAttribute('src')) video.src = `/api/download/${photo.id}?inline=1`;
        },
        toggle,
        seekBy,
        toggleMute,
        pause: () => video.pause(),
        /** Frees the media resources before the slide is dropped */
        release() {
            video.pause();
            video.removeAttribute('src');
            video.load();
        },
    };
}
