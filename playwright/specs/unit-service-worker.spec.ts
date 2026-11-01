// Unit tests of how the page follows the service worker's updates (the worker itself: service-worker.spec.ts).
import { expect, test } from '@playwright/test';
import { watchForUpdate } from '../../assets/js/core/service-worker.js';

class Target {
    private listeners = new Map<string, (() => void)[]>();
    addEventListener(type: string, listener: () => void) {
        this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
    }
    emit(type: string) {
        this.listeners.get(type)?.forEach((listener) => listener());
    }
}

class FakeWorker extends Target {
    state = 'installing';
    messages: unknown[] = [];
    postMessage(message: unknown) {
        this.messages.push(message);
    }
}

function setup({ controlled }: { controlled: boolean }) {
    const registration = Object.assign(new Target(), { waiting: null as FakeWorker | null, installing: null as FakeWorker | null });
    const container = Object.assign(new Target(), { controller: controlled ? ({} as ServiceWorker) : null });
    const calls = { offers: 0, reloads: 0, accept: () => {} };
    const offerAgain = watchForUpdate(registration as any, container as any, {
        offer: (accept) => {
            calls.offers++;
            calls.accept = accept;
        },
        reload: () => calls.reloads++,
    });
    /** A new worker installs and waits. */
    const install = () => {
        const worker = new FakeWorker();
        registration.installing = worker;
        registration.emit('updatefound');
        worker.state = 'installed';
        registration.installing = null;
        registration.waiting = worker;
        worker.emit('statechange');
        return worker;
    };
    return { registration, container, calls, offerAgain, install };
}

test.describe('service worker updates', () => {
    test('a new worker is offered and takes over when accepted, then the page reloads', () => {
        const { calls, install, container } = setup({ controlled: true });
        const worker = install();
        expect(calls.offers).toBe(1);
        expect(worker.messages).toEqual([]);

        calls.accept();
        expect(worker.messages).toEqual([{ type: 'skip-waiting' }]);
        expect(calls.reloads).toBe(0);

        container.emit('controllerchange');
        container.emit('controllerchange');
        expect(calls.reloads).toBe(1);
    });

    test('the first worker taking over a page is neither offered nor reloaded for', () => {
        const { calls, install, container } = setup({ controlled: false });
        install();
        container.emit('controllerchange');
        expect(calls.offers).toBe(0);
        expect(calls.reloads).toBe(0);
    });

    test('a worker already waiting when the page loads is offered, and again when asked', () => {
        const registration = Object.assign(new Target(), { waiting: new FakeWorker() });
        const container = Object.assign(new Target(), { controller: {} as ServiceWorker });
        let offers = 0;
        const offerAgain = watchForUpdate(registration as any, container as any, { offer: () => offers++, reload: () => {} });
        expect(offers).toBe(1);
        offerAgain();
        expect(offers).toBe(2);
    });

    test('nothing is offered while no worker waits', () => {
        const { calls, offerAgain } = setup({ controlled: true });
        offerAgain();
        expect(calls.offers).toBe(0);
    });
});
