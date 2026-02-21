import { gql } from "@apollo/client";

export const RECORD_DEMAND = gql`
  mutation RecordDemand(
    $query: String!
    $centerLat: Float!
    $centerLng: Float!
    $radiusKm: Float!
  ) {
    recordDemand(
      query: $query
      centerLat: $centerLat
      centerLng: $centerLng
      radiusKm: $radiusKm
    )
  }
`;
