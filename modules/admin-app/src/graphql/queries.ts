import { gql } from "@apollo/client";

// Shared fields across all signal types (from signal_meta_resolvers macro)
const SIGNAL_FIELDS = `
  ... on GqlGatheringSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat
    startsAt endsAt actionUrl organizer isRecurring
  }
  ... on GqlAidSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat
    actionUrl availability isOngoing
  }
  ... on GqlNeedSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat
    urgency whatNeeded actionUrl goal
  }
  ... on GqlNoticeSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat
    severity category effectiveDate sourceAuthority
  }
  ... on GqlTensionSignal {
    id title summary sensitivity confidence location { lat lng precision }
    locationName sourceUrl extractedAt sourceDiversity causeHeat
    severity category whatWouldHelp
  }
`;

export const ME = gql`
  query Me {
    me {
      isAdmin
      phoneNumber
    }
  }
`;

export const ADMIN_DASHBOARD = gql`
  query AdminDashboard($region: String!) {
    adminDashboard(region: $region) {
      totalSignals
      totalStories
      totalActors
      totalSources
      activeSources
      totalTensions
      scoutStatuses {
        regionName
        regionSlug
        lastScouted
        sourcesDue
        running
      }
      signalVolumeByDay {
        day
        gatherings
        aids
        needs
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
        sourceLabel
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

export const ADMIN_REGIONS = gql`
  query AdminRegions {
    adminRegions {
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

export const ADMIN_REGION = gql`
  query AdminRegion($slug: String!) {
    adminRegion(slug: $slug) {
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

export const ADMIN_REGION_SOURCES = gql`
  query AdminRegionSources($regionSlug: String!) {
    adminRegionSources(regionSlug: $regionSlug) {
      id
      url
      canonicalValue
      sourceLabel
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
  query AdminScoutStatus($regionSlug: String!) {
    adminScoutStatus(regionSlug: $regionSlug) {
      regionName
      regionSlug
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
      ${SIGNAL_FIELDS}
    }
  }
`;

export const SIGNAL_DETAIL = gql`
  query Signal($id: UUID!) {
    signal(id: $id) {
      ${SIGNAL_FIELDS}
      ... on GqlGatheringSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlAidSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlNeedSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlNoticeSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
      ... on GqlTensionSignal {
        evidence { id sourceUrl snippet relevance }
        actors { id name actorType }
        story { id headline arc }
      }
    }
  }
`;

export const STORIES = gql`
  query Stories($limit: Int, $status: String) {
    stories(limit: $limit, status: $status) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      status
    }
  }
`;

export const STORIES_IN_BOUNDS = gql`
  query StoriesInBounds($minLat: Float!, $maxLat: Float!, $minLng: Float!, $maxLng: Float!, $limit: Int) {
    storiesInBounds(minLat: $minLat, maxLat: $maxLat, minLng: $minLng, maxLng: $maxLng, limit: $limit) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      status
    }
  }
`;

export const STORY_DETAIL = gql`
  query Story($id: UUID!) {
    story(id: $id) {
      id
      headline
      summary
      arc
      category
      energy
      signalCount
      firstSeen
      lastUpdated
      velocity
      dominantType
      status
      lede
      narrative
      tags {
        slug
        name
      }
    }
  }
`;

export const ACTORS = gql`
  query Actors($city: String!, $limit: Int) {
    actors(city: $city, limit: $limit) {
      id
      name
      actorType
      description
      signalCount
      city
    }
  }
`;

export const ALL_TAGS = gql`
  query Tags($limit: Int) {
    tags(limit: $limit) {
      slug
      name
    }
  }
`;
