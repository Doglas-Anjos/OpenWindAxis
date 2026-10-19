/**
 * The map under the dataset table (ESPEC-INTERFACE §1.1): one pin per dataset
 * with known coordinates, in the dataset's colour, with its name; zoom with
 * the +/− buttons (a whole level), the small +/− below them (a quarter of a
 * level, for a finer framing) or the wheel. OpenStreetMap tiles, as the reference
 * product's map: viewing it sends the visible area to the tile server (the
 * data itself never leaves the browser).
 */
import { useEffect, useRef } from 'react';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';

/** One pin. */
export interface Site {
  key: string | number;
  name: string;
  color: string;
  latitude: number;
  longitude: number;
}

const TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';
const TILE_ATTRIBUTION = '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>';
/** OpenStreetMap's deepest tiles. */
const MAX_ZOOM = 19;
/** The whole world, before any dataset has a site. */
const WORLD_CENTER: L.LatLngTuple = [0, 0];
const WORLD_ZOOM = 1;
/** Zoom of the usual +/− buttons, levels, and of the small ones below them. */
const ZOOM_STEP = 1;
const FINE_ZOOM_STEP = 0.25;
/** Deepest zoom when fitting the pins (a lone site, or several at one place): a few kilometres around the tower. */
const SINGLE_SITE_ZOOM = 11;
/** Room around the pins when several are fitted, px. */
const FIT_PADDING_PX = 40;
/** Pin size and the point of it that sits on the site, px. */
const PIN_SIZE: L.PointTuple = [22, 30];
const PIN_TIP: L.PointTuple = [11, 30];
/** The label sits above the pin. */
const LABEL_OFFSET: L.PointTuple = [0, -30];

/** A pin in `color`: our own drawing. */
const pinSvg = (color: string) =>
  `<svg width="22" height="30" viewBox="0 0 22 30"><path d="M11 29C11 29 2 17.5 2 11a9 9 0 0 1 18 0c0 6.5-9 18-9 18z" fill="${color}" stroke="#ffffff" stroke-width="1.5"/><circle cx="11" cy="11" r="3.5" fill="#ffffff"/></svg>`;

/** A second +/− pair, under Leaflet's, that zooms a quarter of a level. */
function fineZoom(): L.Control {
  const control = new L.Control({ position: 'topleft' });
  control.onAdd = (instance: L.Map) => {
    const bar = L.DomUtil.create('div', 'leaflet-bar leaflet-control fine-zoom');
    for (const [text, title, delta] of [
      ['+', 'Aproximar um pouco (¼ de nível)', FINE_ZOOM_STEP],
      ['−', 'Afastar um pouco (¼ de nível)', -FINE_ZOOM_STEP],
    ] as const) {
      const button = L.DomUtil.create('a', '', bar);
      button.href = '#';
      button.role = 'button';
      button.title = title;
      button.setAttribute('aria-label', title);
      button.textContent = text;
      L.DomEvent.on(button, 'click', (event) => {
        L.DomEvent.preventDefault(event);
        instance.setZoom(instance.getZoom() + delta);
      });
    }
    L.DomEvent.disableClickPropagation(bar);
    return bar;
  };
  return control;
}

/** Colours are ours (brand.ts), but check before writing one into markup. */
const SAFE_COLOR = /^#[0-9a-f]{6}$/i;

export function SiteMap({ sites }: { sites: Site[] }) {
  const element = useRef<HTMLDivElement>(null);
  const map = useRef<L.Map | null>(null);
  const pins = useRef<L.LayerGroup | null>(null);

  useEffect(() => {
    // Zoom levels in quarters, so the fine buttons land between whole levels; the usual buttons still step whole levels.
    const instance = L.map(element.current!, { worldCopyJump: true, zoomSnap: FINE_ZOOM_STEP, zoomDelta: ZOOM_STEP }).setView(WORLD_CENTER, WORLD_ZOOM);
    fineZoom().addTo(instance);
    L.tileLayer(TILE_URL, { maxZoom: MAX_ZOOM, attribution: TILE_ATTRIBUTION }).addTo(instance);
    pins.current = L.layerGroup().addTo(instance);
    map.current = instance;
    // The explorer's divider and the window resize the map's box.
    const resize = new ResizeObserver(() => instance.invalidateSize());
    resize.observe(element.current!);
    return () => {
      resize.disconnect();
      instance.remove();
      map.current = null;
    };
  }, []);

  const key = sites.map((site) => `${site.key}:${site.latitude}:${site.longitude}:${site.color}:${site.name}`).join('|');
  useEffect(() => {
    const instance = map.current;
    const layer = pins.current;
    if (!instance || !layer) return;
    layer.clearLayers();
    for (const site of sites) {
      const color = SAFE_COLOR.test(site.color) ? site.color : '#334155';
      const icon = L.divIcon({ className: 'site-pin', html: pinSvg(color), iconSize: PIN_SIZE, iconAnchor: PIN_TIP });
      // The name comes from a file name: text, never markup.
      const label = document.createElement('span');
      label.textContent = site.name;
      L.marker([site.latitude, site.longitude], { icon, title: site.name })
        .bindTooltip(label, { permanent: true, direction: 'top', offset: LABEL_OFFSET, className: 'site-label' })
        .addTo(layer);
    }
    // Sites at (or near) one place zoom no deeper than a lone site.
    if (sites.length)
      instance.fitBounds(L.latLngBounds(sites.map((site) => [site.latitude, site.longitude])), { padding: [FIT_PADDING_PX, FIT_PADDING_PX], maxZoom: SINGLE_SITE_ZOOM });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return (
    <div className="sitemap">
      <div ref={element} className="leaflet-host" />
      {!sites.length && <div className="sitemap-empty">Nenhum dataset com coordenadas. Informe latitude e longitude em Revisar › Configurar dataset.</div>}
    </div>
  );
}
