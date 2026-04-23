// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import ThemePicker from '$lib/components/ThemePicker.svelte';

describe('ThemePicker', () => {
  it('renders all 4 theme buttons', () => {
    render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect: vi.fn() },
    });
    const buttons = screen.getAllByRole('button');
    expect(buttons.length).toBe(4);
  });

  it('marks selected theme button as aria-pressed', () => {
    render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect: vi.fn() },
    });
    const amberBtn = screen.getByRole('button', { name: /Classic Amber/i });
    expect(amberBtn.getAttribute('aria-pressed')).toBe('true');
  });

  it('other themes are NOT aria-pressed', () => {
    render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect: vi.fn() },
    });
    const orangeBtn = screen.getByRole('button', { name: /Classic Orange/i });
    expect(orangeBtn.getAttribute('aria-pressed')).toBe('false');
  });

  it('calls onSelect with the correct theme ID when clicked', async () => {
    const onSelect = vi.fn();
    render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect },
    });
    const orangeBtn = screen.getByRole('button', { name: /Classic Orange/i });
    await fireEvent.click(orangeBtn);
    expect(onSelect).toHaveBeenCalledWith('classic-orange');
  });

  it('renders correct theme labels', () => {
    render(ThemePicker, {
      props: { selected: 'modern-white', onSelect: vi.fn() },
    });
    expect(screen.getByText('Classic Amber')).toBeTruthy();
    expect(screen.getByText('Classic Orange')).toBeTruthy();
    expect(screen.getByText('Modern White')).toBeTruthy();
    expect(screen.getByText('High Contrast')).toBeTruthy();
  });

  it('is wrapped in a fieldset for keyboard accessibility', () => {
    const { container } = render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect: vi.fn() },
    });
    const fieldset = container.querySelector('fieldset');
    expect(fieldset).toBeTruthy();
    const legend = container.querySelector('legend');
    expect(legend?.textContent).toContain('Theme');
  });

  it('buttons are keyboard-focusable (not divs)', () => {
    render(ThemePicker, {
      props: { selected: 'classic-amber', onSelect: vi.fn() },
    });
    const buttons = screen.getAllByRole('button');
    // Every swatch must be a button element, not a div
    for (const btn of buttons) {
      expect(btn.tagName.toLowerCase()).toBe('button');
    }
  });
});
