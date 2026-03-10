import type {
  ServerContext,
  ListAirportDelaysRequest,
  ListAirportDelaysResponse,
  AirportDelayAlert,
} from '../../../../src/generated/server/worldmonitor/aviation/v1/service_server';
import {
  MONITORED_AIRPORTS,
  FAA_AIRPORTS,
} from '../../../../src/config/airports';
import {
  FAA_URL,
  parseFaaXml,
  toProtoDelayType,
  toProtoSeverity,
  toProtoRegion,
  toProtoSource,
  determineSeverity,
  generateSimulatedDelay,
  fetchNotamClosures,
  buildNotamAlert,
} from './_shared';
import { CHROME_UA } from '../../../_shared/constants';
import { cachedFetchJson, getCachedJson, setCachedJson } from '../../../_shared/redis';

const FAA_CACHE_KEY = 'aviation:delays:faa:v1';
const INTL_CACHE_KEY = 'aviation:delays:intl:v3';
const NOTAM_CACHE_KEY = 'aviation:notam:closures:v1';
const CACHE_TTL = 7200;      // 2h for FAA, intl (real), and NOTAM

export async function listAirportDelays(
  _ctx: ServerContext,
  _req: ListAirportDelaysRequest,
): Promise<ListAirportDelaysResponse> {
  const t0 = Date.now();
  // 1. FAA (US) — seed-first with live fallback
  const SEED_FRESHNESS_MS = 45 * 60 * 1000;
  let faaAlerts: AirportDelayAlert[] = [];
  let faaFromSeed = false;
  try {
    const meta = await getCachedJson('seed-meta:aviation:faa', true) as { fetchedAt?: number } | null;
    const seedAge = meta?.fetchedAt ? t0 - meta.fetchedAt : Infinity;
    const seedData = await getCachedJson(FAA_CACHE_KEY, true) as { alerts: AirportDelayAlert[] } | null;
    if (seedData && Array.isArray(seedData.alerts) && (seedAge < SEED_FRESHNESS_MS || !process.env.SEED_FALLBACK_FAA)) {
      faaAlerts = seedData.alerts
        .map(a => {
          const airport = MONITORED_AIRPORTS.find(ap => ap.iata === a.iata);
          if (!airport) return null;
          if (!a.icao || a.icao === '') {
            return { ...a, icao: airport.icao, name: airport.name, city: airport.city, country: airport.country, location: { latitude: airport.lat, longitude: airport.lon }, region: toProtoRegion(airport.region) };
          }
          return a;
        })
        .filter((a): a is AirportDelayAlert => a !== null);
      faaFromSeed = true;
    }
  } catch {}
  // Live fallback: only reached if seed is missing/stale AND SEED_FALLBACK_FAA is set.
  // Default (no env var): stale seed is still served — no live fetch, no cross-isolate stampede.
  // With env var: cachedFetchJson coalesces within one isolate, but parallel isolates
  // may each fire one FAA request until Redis is populated (~same as pre-seed behavior).
  if (!faaFromSeed) {
  try {
    const result = await cachedFetchJson<{ alerts: AirportDelayAlert[] }>(
      FAA_CACHE_KEY, CACHE_TTL, async () => {
        const alerts: AirportDelayAlert[] = [];
        const faaResponse = await fetch(FAA_URL, {
          headers: { Accept: 'application/xml', 'User-Agent': CHROME_UA },
          signal: AbortSignal.timeout(15_000),
        });

        let faaDelays = new Map<string, { airport: string; reason: string; avgDelay: number; type: string }>();
        if (faaResponse.ok) {
          const xml = await faaResponse.text();
          faaDelays = parseFaaXml(xml);
        }

        for (const iata of FAA_AIRPORTS) {
          const airport = MONITORED_AIRPORTS.find((a) => a.iata === iata);
          if (!airport) continue;
          const faaDelay = faaDelays.get(iata);
          if (faaDelay) {
            alerts.push({
              id: `faa-${iata}`,
              iata,
              icao: airport.icao,
              name: airport.name,
              city: airport.city,
              country: airport.country,
              location: { latitude: airport.lat, longitude: airport.lon },
              region: toProtoRegion(airport.region),
              delayType: toProtoDelayType(faaDelay.type),
              severity: toProtoSeverity(determineSeverity(faaDelay.avgDelay)),
              avgDelayMinutes: faaDelay.avgDelay,
              delayedFlightsPct: 0,
              cancelledFlights: 0,
              totalFlights: 0,
              reason: faaDelay.reason,
              source: toProtoSource('faa'),
              updatedAt: Date.now(),
            });
          }
        }

        return { alerts };
      }
    );
    faaAlerts = result?.alerts ?? [];
  } catch (err) {
    console.warn(`[Aviation] FAA fetch failed: ${err instanceof Error ? err.message : 'unknown'}`);
  }
  }

  // 2. International — read-only from Redis (Railway relay seeds the cache)
  let intlAlerts: AirportDelayAlert[] = [];
  try {
    const cached = await getCachedJson(INTL_CACHE_KEY) as { alerts: AirportDelayAlert[] } | null;
    if (cached?.alerts) {
      intlAlerts = cached.alerts;
    } else {
      const nonUs = MONITORED_AIRPORTS.filter(a => a.country !== 'USA');
      intlAlerts = nonUs.map(a => generateSimulatedDelay(a)).filter(Boolean) as AirportDelayAlert[];
    }
  } catch (err) {
    console.warn(`[Aviation] Intl fetch failed: ${err instanceof Error ? err.message : 'unknown'}`);
  }

  // 3. NOTAM closures — seed-first with live fallback
  let allAlerts = [...faaAlerts, ...intlAlerts];
  let notamResult: { closedIcaos: string[]; reasons: Record<string, string> } | null = null;
  let notamFromSeed = false;
  try {
    const notamMeta = await getCachedJson('seed-meta:aviation:notam', true) as { fetchedAt?: number } | null;
    const notamAge = notamMeta?.fetchedAt ? t0 - notamMeta.fetchedAt : Infinity;
    const seedNotam = await getCachedJson(NOTAM_CACHE_KEY, true) as { closedIcaos: string[]; reasons: Record<string, string> } | null;
    if (seedNotam && (notamAge < SEED_FRESHNESS_MS || !process.env.SEED_FALLBACK_NOTAM)) {
      notamResult = seedNotam;
      notamFromSeed = true;
    }
  } catch {}
  // Same stampede-safe design as FAA above: no live fetch unless SEED_FALLBACK_NOTAM is set.
  if (!notamFromSeed && process.env.ICAO_API_KEY) {
    try {
      notamResult = await cachedFetchJson<{ closedIcaos: string[]; reasons: Record<string, string> }>(
        NOTAM_CACHE_KEY, CACHE_TTL, async () => {
          const mena = MONITORED_AIRPORTS.filter(a => a.region === 'mena');
          const result = await fetchNotamClosures(mena);
          const closedIcaos = [...result.closedIcaoCodes];
          const reasons: Record<string, string> = {};
          for (const [icao, reason] of result.notamsByIcao) reasons[icao] = reason;
          return { closedIcaos, reasons };
        }
      );
    } catch (err) {
      console.warn(`[Aviation] NOTAM fetch failed: ${err instanceof Error ? err.message : 'unknown'}`);
    }
  }
  if (notamResult && notamResult.closedIcaos?.length > 0) {
    const existingIatas = new Set(allAlerts.map(a => a.iata));
    for (const icao of notamResult.closedIcaos) {
      const airport = MONITORED_AIRPORTS.find(a => a.icao === icao);
      if (!airport) continue;
      const reason = notamResult.reasons[icao] || 'Airport closure (NOTAM)';
      if (existingIatas.has(airport.iata)) {
        const idx = allAlerts.findIndex(a => a.iata === airport.iata);
        if (idx >= 0) {
          allAlerts[idx] = buildNotamAlert(airport, reason);
        }
      } else {
        allAlerts.push(buildNotamAlert(airport, reason));
      }
    }
    console.warn(`[Aviation] NOTAM: ${notamResult.closedIcaos.length} closures applied`);
  }

  // 4. Fill in ALL monitored airports with no alerts as "normal operations"
  //    so they always appear on the map (gray dots)
  const alertedIatas = new Set(allAlerts.map(a => a.iata));
  let normalCount = 0;
  for (const airport of MONITORED_AIRPORTS) {
    if (!alertedIatas.has(airport.iata)) {
      normalCount++;
      allAlerts.push({
        id: `status-${airport.iata}`,
        iata: airport.iata,
        icao: airport.icao,
        name: airport.name,
        city: airport.city,
        country: airport.country,
        location: { latitude: airport.lat, longitude: airport.lon },
        region: toProtoRegion(airport.region),
        delayType: toProtoDelayType('general'),
        severity: toProtoSeverity('normal'),
        avgDelayMinutes: 0,
        delayedFlightsPct: 0,
        cancelledFlights: 0,
        totalFlights: 0,
        reason: 'Normal operations',
        source: toProtoSource('computed'),
        updatedAt: Date.now(),
      });
    }
  }

  // Write bootstrap key for initial page load hydration
  try {
    await setCachedJson('aviation:delays-bootstrap:v1', { alerts: allAlerts }, 7200);
  } catch { /* non-critical */ }

  return { alerts: allAlerts };
}

