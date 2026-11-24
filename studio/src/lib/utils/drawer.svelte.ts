import type { Component } from 'svelte';

export type DrawerSize = 'sm' | 'md' | 'lg' | 'xl' | 'full';
export type DrawerPosition = 'left' | 'right';

export type DrawerOptions<P = Record<string, any>> = {
  props?: P;
  title?: string;
  size?: DrawerSize;
  position?: DrawerPosition;
  dismissible?: boolean;
  onDismiss?: () => void;
};

export type DrawerInstance<P = Record<string, any>> = {
  id: string;
  component: Component<P & { onClose: () => void }>;
  options: Required<Pick<DrawerOptions<P>, 'size' | 'position' | 'dismissible'>> &
    DrawerOptions<P>;
};

class DrawerManager {
  instances = $state<DrawerInstance[]>([]);

  open<P extends Record<string, any>>(
    component: Component<P & { onClose: () => void }>,
    options?: DrawerOptions<P>
  ): string {
    const id = Math.random().toString(36).substring(2, 9);
    const instance: DrawerInstance<P> = {
      id,
      component,
      options: {
        size: 'md',
        position: 'right',
        dismissible: true,
        ...options
      }
    };
    this.instances.push(instance as any);
    return id;
  }

  close(id: string) {
    const index = this.instances.findIndex((d) => d.id === id);
    if (index === -1) return;

    const instance = this.instances[index];
    instance.options.onDismiss?.();
    this.instances.splice(index, 1);
  }

  closeAll() {
    [...this.instances].forEach((d) => this.close(d.id));
  }

  get size() {
    return this.instances.length;
  }
}

export const drawer = new DrawerManager();
