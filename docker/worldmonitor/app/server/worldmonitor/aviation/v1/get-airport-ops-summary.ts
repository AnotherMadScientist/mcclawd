import type {
    ServerContext,
    GetAirportOpsSummaryRequest,
    GetAirportOpsSummaryResponse,
    AirportOpsSummary,
    FlightDelaySeverity,
} from '../../../../src/generated/server/worldmonitor/aviation/v1/service_server';
import { MONITORED_AIRPORTS } from '../../../../src/config/airports';
import { cachedFetchJson } from '../../../_shared/redis';
import {
    fetchAviationStackDelays,
    fetchNotamClosures,
    determineSeverity,
    parseStringArray,
    DEFAULT_WATCHED_AIRPORTS,
} from './_shared';

const CACHE_TTL = 300; // 5 minutes

export async function getAirportOpsSummary(
    _ctx: ServerContext,
    req: GetAirportOpsSummaryRequest,
): Promise<GetAirportOpsSummaryResponse> {
    const rawAirports = parseStringArray(req.airports);
    const requested = rawAirports.length > 0
        ? rawAirports.map(a => a.toUpperCase())
        : DEFAULT_WATCHED_AIRPORTS;

    const cacheKey = `aviation:ops-summary:v1:${requested.sort().join(',')}`;
    const now = Date.now();

    try {
        const result = await cachedFetchJson<{ summaries: AirportOpsSummary[] }>(
            cacheKey, CACHE_TTL, async () => {
                const airports = MONITORED_AIRPORTS.filter(a => requested.includes(a.iata));
                const summaries: AirportOpsSummary[] = [];

                // Fetch AviationStack delay data
                let avResult = { alerts: [] as any[], healthy: false };
                try {
                    avResult = await fetchAviationStackDelays(airports);
                } catch { /* graceful degradation */ }

                // Fetch NOTAM closures
                let notamResult = { closedIcaoCodes: new Set<string>(), notamsByIcao: new Map<string, string>() };
                try {
                    notamResult = await fetchNotamClosures(airports);
                } catch { /* graceful degradation */ }

                for (const airport of airports) {
                    const alert = avResult.alerts.find(a => a.iata === airport.iata);
                    const isClosed = notamResult.closedIcaoCodes.has(airport.icao);
                    const notamText = notamResult.notamsByIcao.get(airport.icao);

                    const delayPct = alert?.delayedFlightsPct ?? 0;
                    const avgDelay = alert?.avgDelayMinutes ?? 0;
                    const cancelledFlights = alert?.cancelledFlights ?? 0;
                    const totalFlights = alert?.totalFlights ?? 0;
                    const cancelRate = totalFlights > 0 ? (cancelledFlights / totalFlights) * 100 : 0;

                    const sevStr = isClosed ? 'severe' : determineSeverity(avgDelay, delayPct);
                    const severity = `FLIGHT_DELAY_SEVERITY_${sevStr.toUpperCase()}` as FlightDelaySeverity;

                    const notamFlags: string[] = [];
                    if (isClosed) notamFlags.push('CLOSED');
                    if (notamText) notamFlags.push('NOTAM');

                    const topDelayReasons: string[] = [];
                    if (alert?.reason) topDelayReasons.push(alert.reason);
                    if (isClosed && notamText) topDelayReasons.push(notamText.slice(0, 80));

                    summaries.push({
                        iata: airport.iata,
                        icao: airport.icao,
                        name: airport.name,
                        timezone: 'UTC',
                        delayPct,
                        avgDelayMinutes: avgDelay,
                        cancellationRate: Math.round(cancelRate * 10) / 10,
                        totalFlights,
                        closureStatus: isClosed,
                        notamFlags,
                        severity,
                        topDelayReasons,
                        source: avResult.healthy ? 'aviationstack' : 'simulated',
                        updatedAt: now,
                    });
                }

                // Add requested airports not found in MONITORED_AIRPORTS
                for (const iata of requested) {
                    if (!summaries.find(s => s.iata === iata)) {
                        summaries.push({
                            iata,
                            icao: '',
                            name: iata,
                            timezone: 'UTC',
                            delayPct: 0,
                            avgDelayMinutes: 0,
                            cancellationRate: 0,
                            totalFlights: 0,
                            closureStatus: false,
                            notamFlags: [],
                            severity: 'FLIGHT_DELAY_SEVERITY_NORMAL',
                            topDelayReasons: [],
                            source: 'unknown',
                            updatedAt: now,
                        });
                    }
                }

                return { summaries };
            }
        );

        return { summaries: result?.summaries ?? [], cacheHit: false };
    } catch (err) {
        console.warn(`[Aviation] GetAirportOpsSummary failed: ${err instanceof Error ? err.message : err}`);
        return { summaries: [], cacheHit: false };
    }
}
