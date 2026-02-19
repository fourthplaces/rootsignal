import { gql } from "@apollo/client";

export const ME = gql`
  query Me {
    me {
      isAdmin
      phoneNumber
    }
  }
`;

export const ADMIN_DASHBOARD = gql`
  query AdminDashboard($city: String!) {
    adminDashboard(city: $city) {
      totalSignals
      totalStories
      totalActors
      totalSources
      activeSources
      totalTensions
      scoutStatuses {
        cityName
        citySlug
        lastScouted
        sourcesDue
        running
      }
      signalVolumeByDay {
        day
        events
        gives
        asks
        notices
        tensions
      }
      countByType {
        signalType
        count
      }
      storyCountByArc {
        label
        count
      }
      storyCountByCategory {
        label
        count
      }
      freshnessDistribution {
        label
        count
      }
      confidenceDistribution {
        label
        count
      }
      unmetTensions {
        title
        severity
        category
        whatWouldHelp
      }
      topSources {
        name
        signals
        weight
        emptyRuns
      }
      bottomSources {
        name
        signals
        weight
        emptyRuns
      }
      extractionYield {
        sourceType
        extracted
        survived
        corroborated
        contradicted
      }
      gapStats {
        gapType
        total
        successful
        avgWeight
      }
    }
  }
`;

export const ADMIN_CITIES = gql`
  query AdminCities {
    adminCities {
      slug
      name
      centerLat
      centerLng
      radiusKm
      active
      lastScoutCompletedAt
      scoutRunning
      sourcesDue
    }
  }
`;

export const ADMIN_CITY = gql`
  query AdminCity($slug: String!) {
    adminCity(slug: $slug) {
      slug
      name
      centerLat
      centerLng
      radiusKm
      active
      lastScoutCompletedAt
      scoutRunning
      sourcesDue
    }
  }
`;

export const ADMIN_CITY_SOURCES = gql`
  query AdminCitySources($citySlug: String!) {
    adminCitySources(citySlug: $citySlug) {
      id
      url
      canonicalValue
      sourceType
      weight
      qualityPenalty
      effectiveWeight
      discoveryMethod
      lastScraped
      cadenceHours
      signalsProduced
      active
    }
  }
`;

export const ADMIN_SCOUT_STATUS = gql`
  query AdminScoutStatus($citySlug: String!) {
    adminScoutStatus(citySlug: $citySlug) {
      cityName
      citySlug
      lastScouted
      sourcesDue
      running
    }
  }
`;

export const SIGNALS_NEAR_GEO_JSON = gql`
  query SignalsNearGeoJson(
    $lat: Float!
    $lng: Float!
    $radiusKm: Float!
    $types: [SignalType!]
  ) {
    signalsNearGeoJson(lat: $lat, lng: $lng, radiusKm: $radiusKm, types: $types)
  }
`;

export const SIGNALS_RECENT = gql`
  query SignalsRecent($limit: Int, $types: [SignalType!]) {
    signalsRecent(limit: $limit, types: $types) {
      id
      title
      summary
      signalType
      lat
      lng
      confidence
      createdAt
      city
    }
  }
`;

export const SIGNAL_DETAIL = gql`
  query Signal($id: UUID!) {
    signal(id: $id) {
      id
      title
      summary
      signalType
      lat
      lng
      confidence
      createdAt
      city
      evidence {
        id
        url
        snippet
        sourceType
      }
      actors {
        id
        name
        role
      }
      story {
        id
        title
        arc
      }
    }
  }
`;

export const STORIES = gql`
  query Stories($limit: Int, $status: String) {
    stories(limit: $limit, status: $status) {
      id
      title
      arc
      category
      energy
      signalCount
      createdAt
    }
  }
`;

export const STORY_DETAIL = gql`
  query Story($id: UUID!) {
    story(id: $id) {
      id
      title
      arc
      category
      energy
      summary
      signalCount
      createdAt
    }
  }
`;

export const ACTORS = gql`
  query Actors($city: String!, $limit: Int) {
    actors(city: $city, limit: $limit) {
      id
      name
      role
      storyCount
    }
  }
`;

export const EDITIONS = gql`
  query Editions($city: String!, $limit: Int) {
    editions(city: $city, limit: $limit) {
      id
      title
      publishedAt
      signalCount
    }
  }
`;
